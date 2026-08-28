//! `openerprobe` -- does the champion's opener DECISION still want a Farm
//! build (or any other move it cannot legally make) in rounds 1-3, and only
//! get bounced to something legal by [`legal::legal_moves`], or is the
//! opener now genuinely legal-by-construction?
//!
//! # Background
//!
//! A behaviour census (`bin/behavcensus.rs`) measured the bot's first
//! EXECUTED build by round 3: a Mine 95.6% of the time at 3p, matching
//! humans. A now-deleted scratch binary `buildprobe.rs` once measured the
//! opposite-looking number from a different angle: the bot ATTEMPTS an
//! illegal Farm build in round 1 98.1% of the time, 88.3% of those for want
//! of a free worker. Both numbers can be true at once and say nothing
//! contradictory -- "attempts" there almost certainly meant "the top-ranked
//! candidate among a synthetic, not-legality-filtered move set", which
//! nothing currently ported measures. This binary instruments the real
//! decision instead of guessing: [`weighted::eval::WeightedBot::rank_moves`]
//! scores every LEGAL move at every one of a player's own decision points in
//! rounds 1-3 and this reads off what that ranking says.
//!
//! # Method
//!
//! Self-play mirror match (every seat plays `--weights`), truncated the
//! moment `state.round` exceeds 3 -- nothing after round 3 is read by this
//! probe, so there is no reason to keep simulating a game past it (unlike
//! `behavcensus`, which needs the whole game for its other questions).
//!
//! A decision point counts as "the player's own" iff `Move::EndTurn` is
//! among the legal moves right now -- [`legal::legal_moves`]'s own dispatch
//! (`legal.rs`, `Phase::Actions` branch, `action_moves` always pushes
//! `EndTurn` first) makes that exactly the set of moments a player is
//! choosing among Take/Build/Develop/Pop/... for their own turn, as opposed
//! to answering a pending sub-decision (Bid/Defend/Choose/...) opened by
//! somebody else's move or their own -- those never offer `EndTurn` and
//! would otherwise contaminate the Farm-legality reason breakdown with
//! decisions where a build was never a candidate to begin with, independent
//! of worker/resource state.
//!
//! Three measurements, matching the three questions this probe exists to
//! answer:
//!
//! 1. [`Report::top_kind`] -- the bot's actual top-ranked (= chosen) move at
//!    every such decision point, bucketed by kind (`Build` split further by
//!    what it targets).
//! 2. [`Report::build_rank`] / [`Report::build_presence`] -- for each of the
//!    five build-shaped kinds (farm/mine/urban/military build, wonder
//!    step), the rank position (1 = top pick) of the best-scored LEGAL move
//!    of that kind, whenever one is legal at all. This is the honest
//!    substitute for "would an ILLEGAL move have out-ranked the chosen one":
//!    that counterfactual is not cheaply answerable (scoring an illegal
//!    move means running it through [`apply::apply`], which assumes legality
//!    and is not guarded against being handed a move [`legal::legal_moves`]
//!    would have rejected), so this reads the preference ordering ONLY over
//!    what was actually offered, which is exactly what `rank_moves` computes
//!    for the bot's real choice anyway.
//! 3. [`Report::farm_legal`] / [`Report::farm_illegal`] /
//!    [`Report::farm_illegal_reason`] -- specifically for Farm, is a build
//!    legal at all in rounds 1-3, and when not, why. The "why" is read off
//!    the same facts [`legal::legal_moves`]'s own build loop already
//!    branches on ([`state::PlayerState::workers_free`],
//!    [`costs::build_cost_net`], [`state::PlayerState::civil_actions`]) --
//!    not re-derived independently -- so this can never disagree with the
//!    engine about what made a move illegal.
//! 4. [`Report::pop_legal`] / [`Report::pop_illegal`] /
//!    [`Report::pop_illegal_reason`] / [`Report::pop_rank`] /
//!    [`Report::pop_legal_not_chosen_top_kind`] -- the same three questions
//!    (legal at all? rank when legal? what wins when legal but not chosen?)
//!    for `Move::Pop`/`Move::PopFree`, added 2026-08-26 to settle whether the
//!    bot's flat economy (`analysis/openerprobe_2026-08-26.txt`'s original
//!    Farm findings) is a LEGALITY wall or a PREFERENCE: [`classify_pop_illegal`]
//!    reads its reason off the same gates `legal.rs`'s `action_moves` itself
//!    branches on ([`economy::pop_cost`], [`costs::spare_ca`],
//!    [`costs::civil_life_ca_free`], the round-1 §1.9 early return, and the
//!    two CA-free free-pop arms), never re-derived independently.
//! 5. [`Report::develop_legal`] / [`Report::develop_illegal`] /
//!    [`Report::develop_illegal_reason`] / [`Report::develop_rank`] /
//!    [`Report::develop_legal_not_chosen_top_kind`] -- the same, for
//!    `Move::Develop`, because the 2p bot develops zero technologies in
//!    rounds 1-3 and this answers whether that is a choice or a wall.
//!    [`classify_develop_illegal`] reads its reason off `legal.rs`'s hand-card
//!    loop gates ([`costs::spare_ca`], [`costs::civil_life_ca_free`],
//!    [`effects::science_pact_partners_can_pay`], [`costs::tech_cost_net`]).
//! 6. [`Report::action_points`] -- the ACTION POINTS the chosen move
//!    actually debits at each own-turn decision point (rounds 1-3), measured
//!    as the `civil_actions` / `military_actions` pool delta around
//!    `apply::apply` (the engine's own debit, not a hand-coded cost table)
//!    and reported per player-TURN and per non-EndTurn DECISION. Added
//!    2026-08-27: the 1.06 "decisions per turn" figure is not a points
//!    figure (one decision can debit 0-4 CA or 1-3 MA), and this item is
//!    the one that decides which of the two published brackets for the
//!    bot's CA spend per turn is right
//!    (`analysis/opener_action_points_2026-08-27.txt`).
//!
//! ```text
//! cargo run --profile difftest --bin openerprobe -- \
//!     --games 4000 --players 3 --weights /path/to/champ3p.json --threads 2
//! ```

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::weighted::eval::{load_weights, WeightedBot};
use tta::bots::weighted::weights::Weights;
use tta::costs;
use tta::economy;
use tta::effects;
use tta::game::{self, MOVE_CAP};
use tta::legal;
use tta::moves::Move;
use tta::state::{GameState, PlayerState};
use tta::{CardId, CardType};

// ---------------------------------------------------------------------
// Move classification
// ---------------------------------------------------------------------

/// The bucket a chosen move (item 1) falls into. `Build` is split by what it
/// targets -- the whole question this probe exists to answer is about
/// build KIND, so folding all four into one `Build` bucket would erase it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecisionKind {
    Take,
    BuildFarm,
    BuildMine,
    BuildUrban,
    BuildMilitary,
    Develop,
    Upgrade,
    WonderStep,
    Pop,
    Leader,
    ActionCard,
    /// Ending the turn with civil/military actions still available (or
    /// none) -- broken out of [`DecisionKind::Other`] on its own, not folded
    /// in with the response/military-phase tail, because `end_turn_bias`
    /// (`bots/weighted/weights.rs`) is a per-table-size trained weight and
    /// this probe's whole reason for existing is to check whether a weight
    /// move shows up in the bot's actual decisions.
    EndTurn,
    /// Every other legal move an own-turn decision point can offer
    /// (Revolution, Destroy, BachTheater, the trade moves, ...). Coarse on
    /// purpose: none of these bear on the Farm/opener or EndTurn questions,
    /// and `docs/OPENINGS.txt`'s own convention (`behavcensus.rs`'s
    /// `CivilMoveKind::Other`) already folds an equivalent tail into one
    /// bucket rather than naming every variant.
    Other,
}

/// The four `CardType`s [`legal::legal_moves`]'s build loop can ever attach
/// to a [`Move::Build`] (`cards::CardType::takes_workers`: urban, unit, or
/// production). Exhaustive over `CardType` below, not a wildcard match --
/// `wildcard_enum_match_arm` is denied repo-wide -- so a `CardType` this
/// engine ever starts building through a fifth path fails to compile here
/// instead of silently landing in a catch-all.
fn build_target_kind(k: CardType) -> DecisionKind {
    match k {
        CardType::Farm => DecisionKind::BuildFarm,
        CardType::Mine => DecisionKind::BuildMine,
        CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater => {
            DecisionKind::BuildUrban
        }
        CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => DecisionKind::BuildMilitary,
        CardType::Government
        | CardType::SpecialTech
        | CardType::Wonder
        | CardType::Leader
        | CardType::Action
        | CardType::Tactic
        | CardType::Aggression
        | CardType::War
        | CardType::Pact
        | CardType::Bonus
        | CardType::Territory
        | CardType::Event => {
            panic!("Move::Build targeted non-buildable CardType {k:?} -- legal.rs only ever generates Build for takes_workers() kinds")
        }
    }
}

/// Exhaustive over every [`Move`] variant -- see [`build_target_kind`]'s own
/// note on why a wildcard arm is never acceptable here.
fn decision_kind(mv: Move) -> DecisionKind {
    match mv {
        Move::Take { .. } => DecisionKind::Take,
        Move::Build { card } => build_target_kind(card.kind()),
        Move::Develop { .. } => DecisionKind::Develop,
        Move::Upgrade { .. } => DecisionKind::Upgrade,
        Move::WonderStep { .. } => DecisionKind::WonderStep,
        Move::Pop { .. } | Move::PopFree => DecisionKind::Pop,
        Move::PlayLeader { .. } => DecisionKind::Leader,
        Move::PlayAction { .. } => DecisionKind::ActionCard,
        Move::Revolution { .. }
        | Move::Destroy { .. }
        | Move::PlayTactic { .. }
        | Move::CopyTactic { .. }
        | Move::Aggression { .. }
        | Move::War { .. }
        | Move::OfferPact { .. }
        | Move::CancelPact { .. }
        | Move::PrepareEvent { .. }
        | Move::RemoveLeaderYellow
        | Move::ColumbusColonize { .. }
        | Move::Barbarossa { .. }
        | Move::BachTheater { .. }
        | Move::TradeFoodAsResource
        | Move::TradeResourceAsFood
        | Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDiscard { .. }
        | Move::SendDone
        | Move::Choose { .. }
        | Move::Churchill { .. }
        | Move::PolPass
        | Move::Resign => DecisionKind::Other,
        Move::EndTurn => DecisionKind::EndTurn,
    }
}

/// The five build-shaped kinds item 2 tracks a legal-move rank for. A closed
/// subset of [`DecisionKind`] (everything a "build a farm/mine/urban/unit or
/// pay a wonder step" question could mean), kept as its own enum rather than
/// reusing [`DecisionKind`] directly so [`ALL_BUILD_KINDS`] can enumerate
/// exactly these five with no risk of a future non-build `DecisionKind`
/// variant silently being iterated alongside them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuildKind {
    Farm,
    Mine,
    Urban,
    Military,
    WonderStep,
}

const ALL_BUILD_KINDS: [BuildKind; 5] =
    [BuildKind::Farm, BuildKind::Mine, BuildKind::Urban, BuildKind::Military, BuildKind::WonderStep];

fn build_kind_label(k: BuildKind) -> &'static str {
    match k {
        BuildKind::Farm => "Farm",
        BuildKind::Mine => "Mine",
        BuildKind::Urban => "Urban",
        BuildKind::Military => "Military",
        BuildKind::WonderStep => "WonderStep",
    }
}

/// `Some(k)` iff `mv` is one of the five moves item 2 tracks; exhaustive over
/// every [`Move`] variant for the same reason [`decision_kind`] is.
fn probe_build_kind(mv: Move) -> Option<BuildKind> {
    match mv {
        Move::Build { card } => Some(match card.kind() {
            CardType::Farm => BuildKind::Farm,
            CardType::Mine => BuildKind::Mine,
            CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater => BuildKind::Urban,
            CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => BuildKind::Military,
            CardType::Government
            | CardType::SpecialTech
            | CardType::Wonder
            | CardType::Leader
            | CardType::Action
            | CardType::Tactic
            | CardType::Aggression
            | CardType::War
            | CardType::Pact
            | CardType::Bonus
            | CardType::Territory
            | CardType::Event => {
                panic!("Move::Build targeted non-buildable CardType {:?}", card.kind())
            }
        }),
        Move::WonderStep { .. } => Some(BuildKind::WonderStep),
        Move::Take { .. }
        | Move::Develop { .. }
        | Move::Upgrade { .. }
        | Move::Pop { .. }
        | Move::PopFree
        | Move::Revolution { .. }
        | Move::PlayLeader { .. }
        | Move::PlayAction { .. }
        | Move::Destroy { .. }
        | Move::PlayTactic { .. }
        | Move::CopyTactic { .. }
        | Move::Aggression { .. }
        | Move::War { .. }
        | Move::OfferPact { .. }
        | Move::CancelPact { .. }
        | Move::PrepareEvent { .. }
        | Move::RemoveLeaderYellow
        | Move::ColumbusColonize { .. }
        | Move::Barbarossa { .. }
        | Move::BachTheater { .. }
        | Move::TradeFoodAsResource
        | Move::TradeResourceAsFood
        | Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDiscard { .. }
        | Move::SendDone
        | Move::Choose { .. }
        | Move::Churchill { .. }
        | Move::EndTurn
        | Move::PolPass
        | Move::Resign => None,
    }
}

// ---------------------------------------------------------------------
// Farm-build legality reason (item 3)
// ---------------------------------------------------------------------

/// Why [`Move::Build`] of a Farm-type card is absent from
/// [`legal::legal_moves`] right now, read off the same facts `legal.rs`'s
/// own build loop branches on (`legal.rs:554-590`) rather than re-derived.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FarmIllegalReason {
    /// No Farm-type card is in this player's tableau at all -- there is
    /// nothing to put a worker on, whatever the worker/resource state.
    CardNotInPlay,
    /// A Farm card is owned, but `p.workers_free == 0` -- `legal.rs`'s build
    /// loop is gated on this before it ever looks at cost (`legal.rs:554`),
    /// and per [`apply`]'s own doc, [`Move::Build`] is the only move that
    /// ever decrements it, so a Farm never legal here means nothing this
    /// player has done yet has freed one up.
    FreeWorkerUnavailable,
    /// A free worker exists and the Farm card is owned, but this player has
    /// no civil action left to spend and holds no Civil-Life-style
    /// exemption (`costs::civil_life_ca_free`).
    CivilActionUnavailable,
    /// A free worker, a civil action (or exemption), and the card are all
    /// there, but the resource cost (`costs::build_cost_net`, plus any live
    /// Trade-Routes food-as-resource conversion) is not affordable.
    ResourcesUnaffordable,
}

/// Classifies why a Farm build is not legal for `p` right now. Only called
/// once the caller has already confirmed, from the real
/// [`legal::legal_moves`] output, that no `Move::Build` targets a Farm card
/// -- this never re-decides legality itself, only explains it.
///
/// Second return value: whether `p` owns MORE than one Farm-type card right
/// now. Rounds 1-3 essentially never do (`Agriculture` is the only Age-A
/// Farm card and nothing before round 4 can plausibly have developed
/// `Irrigation` too), so this function reads its reason off the FIRST owned
/// Farm card in tableau order -- the flag makes that simplifying assumption
/// visible in the report rather than silently possibly wrong.
fn classify_farm_illegal(state: &GameState, p: &PlayerState) -> (FarmIllegalReason, bool) {
    let farm_ids: Vec<CardId> = p.techs.of_type(CardType::Farm).map(|(id, _)| id).collect();
    let multi_candidate = farm_ids.len() > 1;
    let Some(&id) = farm_ids.first() else {
        return (FarmIllegalReason::CardNotInPlay, multi_candidate);
    };
    if p.workers_free == 0 {
        return (FarmIllegalReason::FreeWorkerUnavailable, multi_candidate);
    }
    let have_ca = p.civil_actions >= 1;
    let exempt = costs::civil_life_ca_free(p.one_time_discount.build_resources);
    if !have_ca && !exempt {
        return (FarmIllegalReason::CivilActionUnavailable, multi_candidate);
    }
    // Ground truth (the caller's real `legal_moves` check) already says this
    // Farm build is not legal, and the two gates above are clear -- the
    // remaining possibility from `legal.rs:557-584` is `costs::
    // build_cost_net`/affordability (the urban-limit check never applies to
    // Farm: `CardType::Farm.is_urban()` is always `false`). Read the SAME
    // cost function `legal.rs` itself calls, rather than asserted by
    // elimination, so a fourth gate added there later cannot silently mislabel
    // as this one.
    debug_assert!(
        costs::build_cost_net(state, p, id).is_none_or(|cost| {
            let trade_fill = tta_trade_fill(state, p);
            let res = i32::from(p.resources);
            !(res >= cost || (res + trade_fill) >= cost)
        }),
        "classify_farm_illegal called on a Farm build legal.rs would itself accept"
    );
    (FarmIllegalReason::ResourcesUnaffordable, multi_candidate)
}

/// `legal.rs`'s own Trade-Routes-Agreement food-as-resource fill, mirrored
/// (not re-derived): the exact `min(remaining grant, food on hand)` amount a
/// build's resource shortfall can be topped up by (`legal.rs:579-580`).
fn tta_trade_fill(state: &GameState, p: &PlayerState) -> i32 {
    tta::economy::trade_food_as_resource_remaining(state, p).min(i32::from(p.food))
}

// ---------------------------------------------------------------------
// Pop-legality reason (item 4)
// ---------------------------------------------------------------------

/// Why no `Move::Pop`/`Move::PopFree` is offered right now, read off the
/// same four gates `legal.rs`'s `action_moves` itself branches on
/// (`legal.rs:425-517`): the round-1 §1.9 early return (before ANY pop arm
/// is reached), an empty yellow bank (nothing to place), no spendable civil
/// action (and neither CA-free arm live), or insufficient food once a civil
/// action is available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PopIllegalReason {
    /// `state.round == 1` -- `action_moves` returns right after the Take
    /// loop, before it ever reaches the pop arms (`legal.rs:425-427`).
    Round1TakeOnly,
    /// [`economy::pop_cost`] is `None` -- `p.yellow_bank == 0`
    /// ([`economy::pop_cost_base`]), so there is no worker in the bank to
    /// place regardless of civil action or food.
    YellowBankEmpty,
    /// A worker is available (bank nonzero) but `p` has no spendable civil
    /// action ([`costs::spare_ca`]) and holds none of Civil Life's
    /// `pop_food` exemption ([`costs::civil_life_ca_free`]) -- and neither
    /// of the two CA-free free-pop arms (`Stats::free_pop_per_turn` plus
    /// unused Ocean Liners, or a Civil Life discount driving the price to
    /// zero) is live either, so nothing rescues this decision point.
    CivilActionUnavailable,
    /// A civil action (or exemption) is there, but food on hand is below
    /// [`economy::pop_cost`]'s price, and neither CA-free free-pop arm
    /// covers it.
    FoodUnaffordable,
}

/// Classifies why no Pop move is legal for `p` right now. Only called once
/// the caller has already confirmed, from the real [`legal::legal_moves`]
/// output, that no `Move::Pop`/`Move::PopFree` is offered -- this never
/// re-decides legality itself, only explains it. The two CA-free free-pop
/// arms (`legal.rs:494` / `legal.rs:510-517`) are checked and `debug_assert`ed
/// false rather than skipped, so a future arm added there that this reader
/// forgot to mirror fails a debug build instead of silently mislabelling.
fn classify_pop_illegal(state: &GameState, p: &PlayerState) -> PopIllegalReason {
    if state.round == 1 {
        return PopIllegalReason::Round1TakeOnly;
    }
    let Some(cost) = economy::pop_cost(state, p) else {
        return PopIllegalReason::YellowBankEmpty;
    };
    let s = effects::state_stats(state, p);
    let free_pop_arm = s.free_pop_per_turn && !p.ocean_liners_used && p.yellow_bank > 0;
    let civil_life_free_pop_arm = p.yellow_bank > 0
        && p.one_time_discount.pop_food > 0
        && p.food == 0
        && economy::pop_cost_base(p.yellow_bank)
            .is_some_and(|base| u16::from(base) <= p.one_time_discount.pop_food as u16);
    debug_assert!(
        !free_pop_arm && !civil_life_free_pop_arm,
        "classify_pop_illegal called when a CA-free free-pop arm legal.rs itself would accept is live"
    );
    let ca = costs::spare_ca(p);
    let ca_ok = ca >= 1 || costs::civil_life_ca_free(p.one_time_discount.pop_food);
    if !ca_ok {
        return PopIllegalReason::CivilActionUnavailable;
    }
    debug_assert!(
        i32::from(p.food) < cost,
        "classify_pop_illegal called on a Pop legal.rs itself would accept (CA ok, food covers cost)"
    );
    PopIllegalReason::FoodUnaffordable
}

// ---------------------------------------------------------------------
// Develop-legality reason (item 5)
// ---------------------------------------------------------------------

/// Why no `Move::Develop` is offered right now, read off the same gates
/// `legal.rs`'s hand-card loop branches on for every develop-eligible card
/// (`legal.rs:701-736`): the round-1 early return, no develop-eligible card
/// in hand at all, no spendable civil action, the science-pact-partners
/// gate (checked once, globally -- [`effects::science_pact_partners_can_pay`]
/// takes no card id), or insufficient science.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DevelopIllegalReason {
    /// `state.round == 1` -- see [`PopIllegalReason::Round1TakeOnly`].
    Round1TakeOnly,
    /// No card in `p.hand_civil` is a develop-eligible [`CardType`]
    /// ([`develop_eligible_kind`]) -- there is nothing to spend science on,
    /// whatever the civil-action or science state.
    NoEligibleCardInHand,
    /// A develop-eligible card is held, but `p` has no spendable civil
    /// action ([`costs::spare_ca`]) and holds none of Civil Life's
    /// `develop_science` exemption ([`costs::civil_life_ca_free`]). This
    /// gate is the SAME check for every hand card (`legal.rs`'s `ca`/`have_ca`
    /// is computed once per decision point, not per card), so it alone
    /// blocks every develop-eligible card at once.
    CivilActionUnavailable,
    /// A civil action (or exemption) is there, but
    /// [`effects::science_pact_partners_can_pay`] is false -- also a single
    /// per-decision-point gate, not per card, so it too blocks every
    /// develop-eligible card at once.
    SciencePactPartnersUnavailable,
    /// Civil action and science-pact-partners are both clear, but every
    /// develop-eligible card's own [`costs::tech_cost_net`] exceeds `p`'s
    /// science on hand.
    ScienceUnaffordable,
}

/// The develop-eligible [`CardType`]s `legal.rs`'s hand-card loop ever
/// offers a `Move::Develop` for (`legal.rs:701` Government arm plus
/// `legal.rs:728`'s `k.takes_workers() || k == CardType::SpecialTech` guard).
/// Exhaustive over `CardType`, not a wildcard match, for the same reason
/// [`build_target_kind`] is.
fn develop_eligible_kind(k: CardType) -> bool {
    match k {
        CardType::Government
        | CardType::Farm
        | CardType::Mine
        | CardType::Lab
        | CardType::Temple
        | CardType::Library
        | CardType::Arena
        | CardType::Theater
        | CardType::Infantry
        | CardType::Cavalry
        | CardType::Artillery
        | CardType::Air
        | CardType::SpecialTech => true,
        CardType::Wonder
        | CardType::Leader
        | CardType::Action
        | CardType::Tactic
        | CardType::Aggression
        | CardType::War
        | CardType::Pact
        | CardType::Bonus
        | CardType::Territory
        | CardType::Event => false,
    }
}

/// Classifies why no Develop move is legal for `p` right now. Only called
/// once the caller has already confirmed, from the real
/// [`legal::legal_moves`] output, that no `Move::Develop` is offered -- this
/// never re-decides legality itself, only explains it.
///
/// Second return value: whether `p` holds MORE than one develop-eligible
/// card right now -- same disclosure convention as
/// [`classify_farm_illegal`]'s `multi_candidate` flag, because
/// [`DevelopIllegalReason::ScienceUnaffordable`] is only asserted true of
/// EVERY eligible card (checked below), never picked off just the first one.
fn classify_develop_illegal(state: &GameState, p: &PlayerState) -> (DevelopIllegalReason, bool) {
    if state.round == 1 {
        return (DevelopIllegalReason::Round1TakeOnly, false);
    }
    let eligible: Vec<CardId> =
        p.hand_civil.as_slice().iter().copied().filter(|&id| develop_eligible_kind(id.kind())).collect();
    let multi_candidate = eligible.len() > 1;
    if eligible.is_empty() {
        return (DevelopIllegalReason::NoEligibleCardInHand, multi_candidate);
    }
    let ca = costs::spare_ca(p);
    let ca_ok = ca >= 1 || costs::civil_life_ca_free(p.one_time_discount.develop_science);
    if !ca_ok {
        return (DevelopIllegalReason::CivilActionUnavailable, multi_candidate);
    }
    if !effects::science_pact_partners_can_pay(state, p) {
        return (DevelopIllegalReason::SciencePactPartnersUnavailable, multi_candidate);
    }
    debug_assert!(
        eligible.iter().all(|&id| i32::from(p.science) < costs::tech_cost_net(state, p, id).unwrap_or(0)),
        "classify_develop_illegal called when some eligible card's science cost legal.rs itself would accept is affordable"
    );
    (DevelopIllegalReason::ScienceUnaffordable, multi_candidate)
}

// ---------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------

// ---------------------------------------------------------------------
// Action-point accounting (item 6)
// ---------------------------------------------------------------------

/// One game's rounds-1-3 action-point ledger, accumulated at every own-turn
/// decision point (the same `EndTurn`-gated set as the other items).
///
/// `ca` / `ma` are the action points the chosen move ACTUALLY debits,
/// measured as the pool delta the engine itself records in `apply::apply` --
/// never a hand-coded table. `ca_decisions` / `ma_decisions` count the
/// decision points that debited >= 1 CA / >= 1 MA respectively. The
/// per-TURN means are computed in `print_report` using the total own-turn
/// count (`top_kind.end_turn`) as the denominator; the per-DECISION means
/// use the non-EndTurn decision count as the denominator. The two
/// denominators differ: a single turn can hold several decision points, and
/// the whole reason this probe exists is that "decisions per turn"
/// (1.06 at 2p) and "CA per turn" are NOT the same number.
#[derive(Default, Clone, Copy)]
struct ActionPointStats {
    /// Civil action points debited by chosen moves, rounds 1-3.
    ca: u64,
    /// Military action points debited by chosen moves, rounds 1-3.
    ma: u64,
    /// Decision points at which the chosen move debited >= 1 CA.
    ca_decisions: u64,
    /// Decision points at which the chosen move debited >= 1 MA.
    ma_decisions: u64,
    /// CA debited in round 1 (diagnostics: the §1.9 round-1 Take-only window
    /// caps the opener at ~1 CA + the take itself).
    ca_round1: u64,
    /// CA debited in round 2.
    ca_round2: u64,
    /// CA debited in round 3.
    ca_round3: u64,
    /// MA debited in round 1.
    ma_round1: u64,
    /// MA debited in round 2.
    ma_round2: u64,
    /// MA debited in round 3.
    ma_round3: u64,
    /// Own-turns played in round 1 -- one `EndTurn` per turn, so this is the
    /// only correct denominator for the round-1 rows.
    turns_round1: u64,
    /// Own-turns played in round 2.
    turns_round2: u64,
    /// Own-turns played in round 3.
    turns_round3: u64,
}

impl ActionPointStats {
    /// Records one chosen move's debit. `round` is the round at the decision
    /// point (the game truncates at `state.round > 3`, so 1-3 always).
    ///
    /// Turn granularity: `apply`'s own dispatch runs `economy::end_of_turn`
    /// (top-up) for `EndTurn`, which REFILLS the pools after the debit -- so
    /// the pre/post delta is a clean read exactly when the chosen move is
    /// not `EndTurn`. EndTurn itself debits nothing, and a decision point is
    /// its player's whole turn (the next decision point in the same game is
    /// always a DIFFERENT player's: the decider is `state.current_player`
    /// and `EndTurn` is what rotates it), so one call per decision point
    /// with `is_end_turn` disambiguates the per-turn counts without any
    /// player-index plumbing.
    fn record(&mut self, round: u8, chosen: &Move, pools: PoolDelta) {
        let (ca, ma) = if *chosen == Move::EndTurn {
            (0, 0)
        } else {
            (
                (pools.pre_ca as i64 - pools.post_ca as i64).max(0) as u64,
                (pools.pre_ma as i64 - pools.post_ma as i64).max(0) as u64,
            )
        };
        // Two debit arms the delta would miss: a Civil-Life 0-CA build, and
        // PlayAction's MA-funded exception (Breakthrough funding
        // Robespierre). Rounds 1-3 exercise neither -- Civil Life is not
        // developable in the opener window and Robespierre does not appear
        // in Age I -- so the delta alone is authoritative here and no
        // correction term is applied.
        self.ca += ca;
        self.ma += ma;
        if *chosen == Move::EndTurn {
            match round {
                1 => self.turns_round1 += 1,
                2 => self.turns_round2 += 1,
                3 => self.turns_round3 += 1,
                _ => {}
            }
        }
        if ca > 0 {
            self.ca_decisions += 1;
            match round {
                1 => self.ca_round1 += ca,
                2 => self.ca_round2 += ca,
                3 => self.ca_round3 += ca,
                _ => {}
            }
        }
        if ma > 0 {
            self.ma_decisions += 1;
            match round {
                1 => self.ma_round1 += ma,
                2 => self.ma_round2 += ma,
                3 => self.ma_round3 += ma,
                _ => {}
            }
        }
    }

    fn merge(&mut self, o: ActionPointStats) {
        self.ca += o.ca;
        self.ma += o.ma;
        self.ca_decisions += o.ca_decisions;
        self.ma_decisions += o.ma_decisions;
        self.ca_round1 += o.ca_round1;
        self.ca_round2 += o.ca_round2;
        self.ca_round3 += o.ca_round3;
        self.ma_round1 += o.ma_round1;
        self.ma_round2 += o.ma_round2;
        self.ma_round3 += o.ma_round3;
        self.turns_round1 += o.turns_round1;
        self.turns_round2 += o.turns_round2;
        self.turns_round3 += o.turns_round3;
    }
}

/// The decider's own action pools either side of one `game::step`. The debit
/// a move actually cost is `pre - post`; see [`ActionPointStats::record`] for
/// why that delta is the authoritative read in rounds 1-3.
#[derive(Clone, Copy)]
struct PoolDelta {
    pre_ca: i8,
    pre_ma: i8,
    post_ca: i8,
    post_ma: i8,
}

#[derive(Default, Clone, Copy)]
struct DecisionKindCounts {
    take: u64,
    build_farm: u64,
    build_mine: u64,
    build_urban: u64,
    build_military: u64,
    develop: u64,
    upgrade: u64,
    wonder_step: u64,
    pop: u64,
    leader: u64,
    action_card: u64,
    end_turn: u64,
    other: u64,
}

impl DecisionKindCounts {
    fn record(&mut self, k: DecisionKind) {
        match k {
            DecisionKind::Take => self.take += 1,
            DecisionKind::BuildFarm => self.build_farm += 1,
            DecisionKind::BuildMine => self.build_mine += 1,
            DecisionKind::BuildUrban => self.build_urban += 1,
            DecisionKind::BuildMilitary => self.build_military += 1,
            DecisionKind::Develop => self.develop += 1,
            DecisionKind::Upgrade => self.upgrade += 1,
            DecisionKind::WonderStep => self.wonder_step += 1,
            DecisionKind::Pop => self.pop += 1,
            DecisionKind::Leader => self.leader += 1,
            DecisionKind::ActionCard => self.action_card += 1,
            DecisionKind::EndTurn => self.end_turn += 1,
            DecisionKind::Other => self.other += 1,
        }
    }

    fn total(&self) -> u64 {
        self.take
            + self.build_farm
            + self.build_mine
            + self.build_urban
            + self.build_military
            + self.develop
            + self.upgrade
            + self.wonder_step
            + self.pop
            + self.leader
            + self.action_card
            + self.end_turn
            + self.other
    }

    fn merge(&mut self, o: DecisionKindCounts) {
        self.take += o.take;
        self.build_farm += o.build_farm;
        self.build_mine += o.build_mine;
        self.build_urban += o.build_urban;
        self.build_military += o.build_military;
        self.develop += o.develop;
        self.upgrade += o.upgrade;
        self.wonder_step += o.wonder_step;
        self.pop += o.pop;
        self.leader += o.leader;
        self.action_card += o.action_card;
        self.end_turn += o.end_turn;
        self.other += o.other;
    }
}

#[derive(Default, Clone)]
struct BuildRankSamples {
    farm: Vec<u32>,
    mine: Vec<u32>,
    urban: Vec<u32>,
    military: Vec<u32>,
    wonder_step: Vec<u32>,
}

impl BuildRankSamples {
    fn record(&mut self, k: BuildKind, rank: u32) {
        match k {
            BuildKind::Farm => self.farm.push(rank),
            BuildKind::Mine => self.mine.push(rank),
            BuildKind::Urban => self.urban.push(rank),
            BuildKind::Military => self.military.push(rank),
            BuildKind::WonderStep => self.wonder_step.push(rank),
        }
    }

    fn get(&self, k: BuildKind) -> &[u32] {
        match k {
            BuildKind::Farm => &self.farm,
            BuildKind::Mine => &self.mine,
            BuildKind::Urban => &self.urban,
            BuildKind::Military => &self.military,
            BuildKind::WonderStep => &self.wonder_step,
        }
    }

    fn merge(&mut self, mut o: BuildRankSamples) {
        self.farm.append(&mut o.farm);
        self.mine.append(&mut o.mine);
        self.urban.append(&mut o.urban);
        self.military.append(&mut o.military);
        self.wonder_step.append(&mut o.wonder_step);
    }
}

/// Present/absent counts per [`BuildKind`], indexed the same way
/// [`BuildRankSamples`] is (present = at least one legal move of this kind
/// existed at the decision point; its rank landed in the matching
/// [`BuildRankSamples`] field).
#[derive(Default, Clone, Copy)]
struct BuildPresenceCounts {
    farm_present: u64,
    farm_absent: u64,
    mine_present: u64,
    mine_absent: u64,
    urban_present: u64,
    urban_absent: u64,
    military_present: u64,
    military_absent: u64,
    wonder_step_present: u64,
    wonder_step_absent: u64,
}

impl BuildPresenceCounts {
    fn record(&mut self, k: BuildKind, present: bool) {
        match (k, present) {
            (BuildKind::Farm, true) => self.farm_present += 1,
            (BuildKind::Farm, false) => self.farm_absent += 1,
            (BuildKind::Mine, true) => self.mine_present += 1,
            (BuildKind::Mine, false) => self.mine_absent += 1,
            (BuildKind::Urban, true) => self.urban_present += 1,
            (BuildKind::Urban, false) => self.urban_absent += 1,
            (BuildKind::Military, true) => self.military_present += 1,
            (BuildKind::Military, false) => self.military_absent += 1,
            (BuildKind::WonderStep, true) => self.wonder_step_present += 1,
            (BuildKind::WonderStep, false) => self.wonder_step_absent += 1,
        }
    }

    fn counts(&self, k: BuildKind) -> (u64, u64) {
        match k {
            BuildKind::Farm => (self.farm_present, self.farm_absent),
            BuildKind::Mine => (self.mine_present, self.mine_absent),
            BuildKind::Urban => (self.urban_present, self.urban_absent),
            BuildKind::Military => (self.military_present, self.military_absent),
            BuildKind::WonderStep => (self.wonder_step_present, self.wonder_step_absent),
        }
    }

    fn merge(&mut self, o: BuildPresenceCounts) {
        self.farm_present += o.farm_present;
        self.farm_absent += o.farm_absent;
        self.mine_present += o.mine_present;
        self.mine_absent += o.mine_absent;
        self.urban_present += o.urban_present;
        self.urban_absent += o.urban_absent;
        self.military_present += o.military_present;
        self.military_absent += o.military_absent;
        self.wonder_step_present += o.wonder_step_present;
        self.wonder_step_absent += o.wonder_step_absent;
    }
}

#[derive(Default, Clone, Copy)]
struct FarmReasonCounts {
    no_such_card_in_play: u64,
    no_free_worker: u64,
    no_civil_action: u64,
    no_resources: u64,
}

impl FarmReasonCounts {
    fn record(&mut self, r: FarmIllegalReason) {
        match r {
            FarmIllegalReason::CardNotInPlay => self.no_such_card_in_play += 1,
            FarmIllegalReason::FreeWorkerUnavailable => self.no_free_worker += 1,
            FarmIllegalReason::CivilActionUnavailable => self.no_civil_action += 1,
            FarmIllegalReason::ResourcesUnaffordable => self.no_resources += 1,
        }
    }

    fn merge(&mut self, o: FarmReasonCounts) {
        self.no_such_card_in_play += o.no_such_card_in_play;
        self.no_free_worker += o.no_free_worker;
        self.no_civil_action += o.no_civil_action;
        self.no_resources += o.no_resources;
    }
}

#[derive(Default, Clone, Copy)]
struct PopReasonCounts {
    round1_take_only: u64,
    yellow_bank_empty: u64,
    no_civil_action: u64,
    no_food: u64,
}

impl PopReasonCounts {
    fn record(&mut self, r: PopIllegalReason) {
        match r {
            PopIllegalReason::Round1TakeOnly => self.round1_take_only += 1,
            PopIllegalReason::YellowBankEmpty => self.yellow_bank_empty += 1,
            PopIllegalReason::CivilActionUnavailable => self.no_civil_action += 1,
            PopIllegalReason::FoodUnaffordable => self.no_food += 1,
        }
    }

    fn merge(&mut self, o: PopReasonCounts) {
        self.round1_take_only += o.round1_take_only;
        self.yellow_bank_empty += o.yellow_bank_empty;
        self.no_civil_action += o.no_civil_action;
        self.no_food += o.no_food;
    }
}

#[derive(Default, Clone, Copy)]
struct DevelopReasonCounts {
    round1_take_only: u64,
    no_eligible_card: u64,
    no_civil_action: u64,
    no_science_pact_partners: u64,
    no_science: u64,
}

impl DevelopReasonCounts {
    fn record(&mut self, r: DevelopIllegalReason) {
        match r {
            DevelopIllegalReason::Round1TakeOnly => self.round1_take_only += 1,
            DevelopIllegalReason::NoEligibleCardInHand => self.no_eligible_card += 1,
            DevelopIllegalReason::CivilActionUnavailable => self.no_civil_action += 1,
            DevelopIllegalReason::SciencePactPartnersUnavailable => self.no_science_pact_partners += 1,
            DevelopIllegalReason::ScienceUnaffordable => self.no_science += 1,
        }
    }

    fn merge(&mut self, o: DevelopReasonCounts) {
        self.round1_take_only += o.round1_take_only;
        self.no_eligible_card += o.no_eligible_card;
        self.no_civil_action += o.no_civil_action;
        self.no_science_pact_partners += o.no_science_pact_partners;
        self.no_science += o.no_science;
    }
}

#[derive(Default)]
struct Report {
    games: u64,
    /// Own-turn decision points in rounds 1-3, summed over every player and
    /// every game (see this file's top doc comment for the `EndTurn`-based
    /// "own decision point" gate).
    decisions: u64,

    /// Item 6: action points debited by the chosen move at each own-turn
    /// decision point, rounds 1-3 -- see [`ActionPointStats`].
    action_points: ActionPointStats,

    top_kind: DecisionKindCounts,
    build_rank: BuildRankSamples,
    build_presence: BuildPresenceCounts,

    farm_legal: u64,
    farm_illegal: u64,
    farm_illegal_reason: FarmReasonCounts,
    /// How many of the `farm_illegal` decisions were classified off a
    /// tableau holding more than one Farm-type card -- see
    /// `classify_farm_illegal`'s own doc comment for why a nonzero count
    /// here means its single-candidate simplification was exercised.
    farm_illegal_multi_candidate: u64,

    /// Item 4: Pop legality/rank/instead-of -- see this file's top doc
    /// comment, item 4.
    pop_legal: u64,
    pop_illegal: u64,
    pop_illegal_reason: PopReasonCounts,
    /// Rank of the best-scored legal Pop move among every legal move at a
    /// decision point where one was legal (1 = the bot's actual top pick).
    pop_rank: Vec<u32>,
    /// When Pop was legal but NOT the top-ranked (chosen) move, the kind of
    /// move that WAS chosen instead.
    pop_legal_not_chosen_top_kind: DecisionKindCounts,

    /// Item 5: Develop legality/rank/instead-of -- see this file's top doc
    /// comment, item 5.
    develop_legal: u64,
    develop_illegal: u64,
    develop_illegal_reason: DevelopReasonCounts,
    /// How many of the `develop_illegal` decisions were classified while
    /// holding more than one develop-eligible card -- see
    /// `classify_develop_illegal`'s own doc comment.
    develop_illegal_multi_candidate: u64,
    develop_rank: Vec<u32>,
    develop_legal_not_chosen_top_kind: DecisionKindCounts,
}

impl Report {
    fn merge(&mut self, mut o: Report) {
        self.games += o.games;
        self.decisions += o.decisions;
        self.action_points.merge(o.action_points);
        self.top_kind.merge(o.top_kind);
        self.build_rank.merge(o.build_rank);
        self.build_presence.merge(o.build_presence);
        self.farm_legal += o.farm_legal;
        self.farm_illegal += o.farm_illegal;
        self.farm_illegal_reason.merge(o.farm_illegal_reason);
        self.farm_illegal_multi_candidate += o.farm_illegal_multi_candidate;

        self.pop_legal += o.pop_legal;
        self.pop_illegal += o.pop_illegal;
        self.pop_illegal_reason.merge(o.pop_illegal_reason);
        self.pop_rank.append(&mut o.pop_rank);
        self.pop_legal_not_chosen_top_kind.merge(o.pop_legal_not_chosen_top_kind);

        self.develop_legal += o.develop_legal;
        self.develop_illegal += o.develop_illegal;
        self.develop_illegal_reason.merge(o.develop_illegal_reason);
        self.develop_illegal_multi_candidate += o.develop_illegal_multi_candidate;
        self.develop_rank.append(&mut o.develop_rank);
        self.develop_legal_not_chosen_top_kind.merge(o.develop_legal_not_chosen_top_kind);
    }
}

fn percentiles_u32(mut v: Vec<u32>) -> String {
    if v.is_empty() {
        return "n/a (no samples)".to_string();
    }
    v.sort_unstable();
    let at = |p: f64| -> u32 {
        let i = ((v.len() - 1) as f64 * p).round() as usize;
        v[i]
    };
    let mean: f64 = v.iter().map(|&x| f64::from(x)).sum::<f64>() / v.len() as f64;
    format!(
        "min={} p25={} median={} p75={} max={} mean={:.2} n={}",
        v[0],
        at(0.25),
        at(0.50),
        at(0.75),
        v[v.len() - 1],
        mean,
        v.len()
    )
}

// ---------------------------------------------------------------------
// One game
// ---------------------------------------------------------------------

fn record_decision(
    report: &mut Report,
    state: &GameState,
    idx: u8,
    legal: &[Move],
    ranked: &[(Move, f64)],
) {
    report.decisions += 1;

    // Item 1: the chosen move's kind. `ranked[0].0 == WeightedBot::choose`'s
    // own pick, by `rank_moves`'s own contract (`eval.rs`).
    report.top_kind.record(decision_kind(ranked[0].0));

    // Item 2: for each build-shaped kind, is it legal here at all, and if so
    // what rank does the best-scored one of that kind hold among every
    // legal move at this decision point (1 = the bot's actual top pick).
    for k in ALL_BUILD_KINDS {
        match ranked.iter().position(|&(m, _)| probe_build_kind(m) == Some(k)) {
            Some(pos) => {
                report.build_presence.record(k, true);
                report.build_rank.record(k, (pos + 1) as u32);
            }
            None => report.build_presence.record(k, false),
        }
    }

    // Item 3: Farm legality and, when illegal, why -- read straight off the
    // real legal-move list, never re-derived independently of it.
    let p = &state.players[idx as usize];
    let farm_legal = legal.iter().any(|m| matches!(m, Move::Build { card } if card.kind() == CardType::Farm));
    if farm_legal {
        report.farm_legal += 1;
    } else {
        report.farm_illegal += 1;
        let (reason, multi) = classify_farm_illegal(state, p);
        report.farm_illegal_reason.record(reason);
        if multi {
            report.farm_illegal_multi_candidate += 1;
        }
    }

    // Item 4: Pop legality (and why not), rank when legal, and what wins
    // when legal but not chosen.
    let pop_legal = legal.iter().any(|m| decision_kind(*m) == DecisionKind::Pop);
    if pop_legal {
        report.pop_legal += 1;
        let pos = ranked
            .iter()
            .position(|&(m, _)| decision_kind(m) == DecisionKind::Pop)
            .expect("pop_legal true implies `ranked` (the same legal set, scored) holds a Pop-kind move");
        report.pop_rank.push((pos + 1) as u32);
        let top_kind = decision_kind(ranked[0].0);
        if top_kind != DecisionKind::Pop {
            report.pop_legal_not_chosen_top_kind.record(top_kind);
        }
    } else {
        report.pop_illegal += 1;
        report.pop_illegal_reason.record(classify_pop_illegal(state, p));
    }

    // Item 5: same three questions for Develop.
    let develop_legal = legal.iter().any(|m| decision_kind(*m) == DecisionKind::Develop);
    if develop_legal {
        report.develop_legal += 1;
        let pos = ranked
            .iter()
            .position(|&(m, _)| decision_kind(m) == DecisionKind::Develop)
            .expect("develop_legal true implies `ranked` (the same legal set, scored) holds a Develop-kind move");
        report.develop_rank.push((pos + 1) as u32);
        let top_kind = decision_kind(ranked[0].0);
        if top_kind != DecisionKind::Develop {
            report.develop_legal_not_chosen_top_kind.record(top_kind);
        }
    } else {
        report.develop_illegal += 1;
        let (reason, multi) = classify_develop_illegal(state, p);
        report.develop_illegal_reason.record(reason);
        if multi {
            report.develop_illegal_multi_candidate += 1;
        }
    }
}

/// Plays one self-play mirror-match game, truncated the moment
/// `state.round` exceeds 3 -- see this file's top doc comment.
fn play_one(players: u8, weights: Weights, seed: u64) -> Report {
    let bots: Vec<WeightedBot> = (0..players).map(|_| WeightedBot::new(weights)).collect();
    let mut state = game::new_game(players, seed);
    let mut report = Report { games: 1, ..Report::default() };

    let mut moves_played = 0usize;
    while !state.game_over && state.round <= 3 {
        if moves_played >= MOVE_CAP {
            break;
        }
        let idx = state.decider();
        let legal = legal::legal_moves(&state);
        // See this file's top doc comment: `EndTurn` is offered only on the
        // decider's own action-phase turn, never while answering a pending
        // sub-decision opened by any player's move.
        let is_own_turn = legal.as_slice().iter().any(|m| matches!(m, Move::EndTurn));
        // Item 6: snapshot the decider's pools BEFORE this move's
        // `game::step` runs, and the round at the moment of the decision
        // (the round may advance inside this step's `advance_turn`, so the
        // post-step `state.round` would mis-bucket the final move of a
        // round). The "post" pools are read after the step returns.
        let pre = &state.players[idx as usize];
        let (pre_ca, pre_ma) = (pre.civil_actions, pre.military_actions);
        let round_at_decide = state.round;
        // Items 1-5 read the state AS THE DECIDER SAW IT, so they must run
        // before the step -- `legal` and `ranked` describe the pre-move
        // position and classifying them against the post-move board
        // misattributes every legality reason.
        let mv = if is_own_turn {
            let ranked = bots[idx as usize].rank_moves(&state, legal.as_slice());
            record_decision(&mut report, &state, idx, legal.as_slice(), &ranked);
            ranked[0].0
        } else {
            bots[idx as usize].choose(&state, legal.as_slice())
        };
        game::step(&mut state, mv);
        // Item 6 is the only thing that needs the post-move pools.
        if is_own_turn {
            let post = &state.players[idx as usize];
            report.action_points.record(
                round_at_decide.min(3) as u8,
                &mv,
                PoolDelta {
                    pre_ca,
                    pre_ma,
                    post_ca: post.civil_actions,
                    post_ma: post.military_actions,
                },
            );
        }
        moves_played += 1;
    }
    report
}

// ---------------------------------------------------------------------
// CLI (same shape as `bin/behavcensus.rs`)
// ---------------------------------------------------------------------

struct Args {
    games: usize,
    players: u8,
    weights_path: String,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args { games: 20, players: 3, weights_path: String::new(), seed: 1, threads: 1 }
    }
}

const USAGE: &str = "\
usage: openerprobe --weights PATH [options]

  --games N       games to play (default 20)
  --players N     2, 3 or 4 (default 3)
  --weights PATH  champion JSON every seat plays (required)
  --seed N        base seed; game g uses seed+g (default 1)
  --threads N     games in parallel (default 1)
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--games" => a.games = value(flag)?.parse().map_err(|_| "bad --games".to_string())?,
            "--players" => a.players = value(flag)?.parse().map_err(|_| "bad --players".to_string())?,
            "--weights" => a.weights_path = value(flag)?,
            "--seed" => a.seed = value(flag)?.parse().map_err(|_| "bad --seed".to_string())?,
            "--threads" => a.threads = value(flag)?.parse().map_err(|_| "bad --threads".to_string())?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    if !(2..=4).contains(&a.players) {
        return Err(format!("--players must be 2, 3 or 4, got {}", a.players));
    }
    if a.weights_path.is_empty() {
        return Err("--weights is required".to_string());
    }
    if a.games == 0 {
        return Err("--games must be at least 1".to_string());
    }
    if a.threads == 0 {
        a.threads = 1;
    }
    Ok(Some(a))
}

/// Prints the same 13-row kind breakdown [`Report::top_kind`] originally
/// printed inline as item 1 -- factored out so items 4 and 5's "what wins
/// instead" tables (same [`DecisionKindCounts`] shape, a different
/// condition-on-decision-point) print identically rather than drifting.
fn print_kind_counts(t: &DecisionKindCounts) {
    let total = t.total().max(1);
    let rows: [(&str, u64); 13] = [
        ("Take", t.take),
        ("Build Farm", t.build_farm),
        ("Build Mine", t.build_mine),
        ("Build Urban", t.build_urban),
        ("Build Military", t.build_military),
        ("Develop", t.develop),
        ("Upgrade", t.upgrade),
        ("WonderStep", t.wonder_step),
        ("Pop", t.pop),
        ("Leader", t.leader),
        ("ActionCard", t.action_card),
        ("EndTurn", t.end_turn),
        ("Other", t.other),
    ];
    for (name, n) in rows {
        println!("- {name}: {n}/{total} ({:.1}%)", 100.0 * n as f64 / total as f64);
    }
}

fn print_report(players: u8, r: &Report) {
    println!("\n## {players}p (n={} games, {} own-turn decision points in rounds 1-3)\n", r.games, r.decisions);

    println!("### Item 1: top-ranked (chosen) move by kind\n");
    print_kind_counts(&r.top_kind);

    println!("\n### Item 2: rank of the best LEGAL move of each build-shaped kind\n");
    println!("(rank 1 = the bot's actual top pick at that decision point; only counted when legal)\n");
    for k in ALL_BUILD_KINDS {
        let (present, absent) = r.build_presence.counts(k);
        let ranks = r.build_rank.get(k).to_vec();
        println!(
            "- {}: legal at {present}/{} decisions ({:.1}%), illegal at {absent}",
            build_kind_label(k),
            present + absent,
            100.0 * present as f64 / (present + absent).max(1) as f64
        );
        println!("  rank when legal: {}", percentiles_u32(ranks));
    }

    println!("\n### Item 3: Farm build legality in rounds 1-3\n");
    let farm_total = r.farm_legal + r.farm_illegal;
    println!(
        "Legal: {}/{} ({:.1}%)   Illegal: {}/{} ({:.1}%)",
        r.farm_legal,
        farm_total.max(1),
        100.0 * r.farm_legal as f64 / farm_total.max(1) as f64,
        r.farm_illegal,
        farm_total.max(1),
        100.0 * r.farm_illegal as f64 / farm_total.max(1) as f64
    );
    if r.farm_illegal > 0 {
        let fr = &r.farm_illegal_reason;
        let ft = r.farm_illegal.max(1);
        println!("Why illegal (of {} illegal decisions):", r.farm_illegal);
        println!("- no such Farm card in play: {}/{ft} ({:.1}%)", fr.no_such_card_in_play, 100.0 * fr.no_such_card_in_play as f64 / ft as f64);
        println!("- no free worker: {}/{ft} ({:.1}%)", fr.no_free_worker, 100.0 * fr.no_free_worker as f64 / ft as f64);
        println!("- no civil action: {}/{ft} ({:.1}%)", fr.no_civil_action, 100.0 * fr.no_civil_action as f64 / ft as f64);
        println!("- no resources: {}/{ft} ({:.1}%)", fr.no_resources, 100.0 * fr.no_resources as f64 / ft as f64);
        println!(
            "(of which classified off a tableau with >1 Farm card owned: {}/{ft})",
            r.farm_illegal_multi_candidate
        );
    }

    println!("\n### Item 4: Pop (increase population) legality in rounds 1-3\n");
    let pop_total = (r.pop_legal + r.pop_illegal).max(1);
    println!(
        "Legal: {}/{pop_total} ({:.1}%)   Illegal: {}/{pop_total} ({:.1}%)",
        r.pop_legal,
        100.0 * r.pop_legal as f64 / pop_total as f64,
        r.pop_illegal,
        100.0 * r.pop_illegal as f64 / pop_total as f64
    );
    if r.pop_illegal > 0 {
        let pr = &r.pop_illegal_reason;
        let pt = r.pop_illegal.max(1);
        println!("Why illegal (of {} illegal decisions):", r.pop_illegal);
        println!(
            "- round 1 (taking a card is the only legal action, SS1.9): {}/{pt} ({:.1}%)",
            pr.round1_take_only,
            100.0 * pr.round1_take_only as f64 / pt as f64
        );
        println!(
            "- yellow bank empty (no worker available to place): {}/{pt} ({:.1}%)",
            pr.yellow_bank_empty,
            100.0 * pr.yellow_bank_empty as f64 / pt as f64
        );
        println!(
            "- no civil action available: {}/{pt} ({:.1}%)",
            pr.no_civil_action,
            100.0 * pr.no_civil_action as f64 / pt as f64
        );
        println!("- food unaffordable: {}/{pt} ({:.1}%)", pr.no_food, 100.0 * pr.no_food as f64 / pt as f64);
    }
    println!("\nRank when legal (1 = bot's actual top pick): {}", percentiles_u32(r.pop_rank.clone()));
    println!("\nWhen Pop is legal but NOT the top pick, what wins instead:\n");
    print_kind_counts(&r.pop_legal_not_chosen_top_kind);

    println!("\n### Item 5: Develop legality in rounds 1-3\n");
    let develop_total = (r.develop_legal + r.develop_illegal).max(1);
    println!(
        "Legal: {}/{develop_total} ({:.1}%)   Illegal: {}/{develop_total} ({:.1}%)",
        r.develop_legal,
        100.0 * r.develop_legal as f64 / develop_total as f64,
        r.develop_illegal,
        100.0 * r.develop_illegal as f64 / develop_total as f64
    );
    if r.develop_illegal > 0 {
        let dr = &r.develop_illegal_reason;
        let dt = r.develop_illegal.max(1);
        println!("Why illegal (of {} illegal decisions):", r.develop_illegal);
        println!(
            "- round 1 (taking a card is the only legal action, SS1.9): {}/{dt} ({:.1}%)",
            dr.round1_take_only,
            100.0 * dr.round1_take_only as f64 / dt as f64
        );
        println!(
            "- no develop-eligible card in hand: {}/{dt} ({:.1}%)",
            dr.no_eligible_card,
            100.0 * dr.no_eligible_card as f64 / dt as f64
        );
        println!(
            "- no civil action available: {}/{dt} ({:.1}%)",
            dr.no_civil_action,
            100.0 * dr.no_civil_action as f64 / dt as f64
        );
        println!(
            "- science-pact partner(s) cannot pay their share: {}/{dt} ({:.1}%)",
            dr.no_science_pact_partners,
            100.0 * dr.no_science_pact_partners as f64 / dt as f64
        );
        println!(
            "- science unaffordable (every eligible card): {}/{dt} ({:.1}%)",
            dr.no_science,
            100.0 * dr.no_science as f64 / dt as f64
        );
        println!(
            "(of which classified while holding >1 develop-eligible card: {}/{dt})",
            r.develop_illegal_multi_candidate
        );
    }
    println!("\nRank when legal (1 = bot's actual top pick): {}", percentiles_u32(r.develop_rank.clone()));
    println!("\nWhen Develop is legal but NOT the top pick, what wins instead:\n");
    print_kind_counts(&r.develop_legal_not_chosen_top_kind);

    // Item 6: action points actually debited by the chosen move, per turn
    // and per non-EndTurn decision (see `ActionPointStats` for the unit
    // distinction this section exists to keep visible).
    let ap = &r.action_points;
    let non_et = (r.top_kind.total() - r.top_kind.end_turn) as f64;
    println!("\n### Item 6: action points debited by the chosen move, rounds 1-3\n");
    println!("(CA/MA = the decider's own pool deltas around apply::apply -- the engine's debit itself)");
    println!(
        "- total: {} CA / {} MA debited over {} own-turn decision points",
        ap.ca,
        ap.ma,
        r.decisions
    );
    let own_turns = r.top_kind.end_turn;
    println!(
        "- per player-TURN (denominator = {} own-turns):  CA {ca:.3}/turn   MA {ma:.3}/turn   total {t:.3}/turn",
        own_turns,
        ca = ap.ca as f64 / own_turns.max(1) as f64,
        ma = ap.ma as f64 / own_turns.max(1) as f64,
        t = (ap.ca + ap.ma) as f64 / own_turns.max(1) as f64
    );
    println!(
        "- per non-EndTurn DECISION:  CA {ca:.3}/decision   MA {ma:.3}/decision   total {t:.3}/decision   (n = {n} non-EndTurn decisions; CA debiting {cd}, MA debiting {md})",
        ca = ap.ca as f64 / non_et.max(1.0),
        ma = ap.ma as f64 / non_et.max(1.0),
        t = (ap.ca + ap.ma) as f64 / non_et.max(1.0),
        n = r.top_kind.total() - r.top_kind.end_turn,
        cd = ap.ca_decisions,
        md = ap.ma_decisions
    );
    // Each round's own denominator: the own-turns played IN that round, so
    // these three rows are a decomposition of the per-turn headline above and
    // not a second, silently different rate.
    let (t1, t2, t3) = (
        ap.turns_round1.max(1) as f64,
        ap.turns_round2.max(1) as f64,
        ap.turns_round3.max(1) as f64,
    );
    println!(
        "- CA per turn by round:  r1 {:.3}   r2 {:.3}   r3 {:.3}   (turns {} / {} / {})",
        ap.ca_round1 as f64 / t1,
        ap.ca_round2 as f64 / t2,
        ap.ca_round3 as f64 / t3,
        ap.turns_round1,
        ap.turns_round2,
        ap.turns_round3
    );
    println!(
        "- MA per turn by round:  r1 {:.3}   r2 {:.3}   r3 {:.3}",
        ap.ma_round1 as f64 / t1,
        ap.ma_round2 as f64 / t2,
        ap.ma_round3 as f64 / t3
    );
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openerprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let weights = match load_weights(std::path::Path::new(&args.weights_path)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("openerprobe: loading {}: {e}", args.weights_path);
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let mut results: Vec<Option<Report>> = (0..args.games).map(|_| None).collect();

    std::thread::scope(|scope| {
        let (slots, args, next, weights) = (&mut results[..], &args, &next, &weights);
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(move || {
                let mut mine = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= args.games {
                        break;
                    }
                    let seed = args.seed.wrapping_add(i as u64);
                    let r = play_one(args.players, *weights, seed);
                    mine.push((i, r));
                }
                mine
            }));
        }
        for h in handles {
            for (i, r) in h.join().expect("openerprobe thread panicked") {
                slots[i] = Some(r);
            }
        }
    });

    let mut overall = Report::default();
    for r in results {
        overall.merge(r.expect("every game played"));
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("games        {}", args.games);
    println!("players      {}", args.players);
    println!("weights      {}", args.weights_path);
    println!("seeds        {}..{}", args.seed, args.seed + args.games as u64 - 1);
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", args.games as f64 / elapsed.max(1e-9));

    print_report(args.players, &overall);

    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_target_kind_maps_every_takes_workers_cardtype_to_a_distinct_bucket() {
        assert_eq!(build_target_kind(CardType::Farm), DecisionKind::BuildFarm);
        assert_eq!(build_target_kind(CardType::Mine), DecisionKind::BuildMine);
        assert_eq!(build_target_kind(CardType::Lab), DecisionKind::BuildUrban);
        assert_eq!(build_target_kind(CardType::Theater), DecisionKind::BuildUrban);
        assert_eq!(build_target_kind(CardType::Infantry), DecisionKind::BuildMilitary);
        assert_eq!(build_target_kind(CardType::Air), DecisionKind::BuildMilitary);
    }

    #[test]
    #[should_panic(expected = "non-buildable CardType")]
    fn build_target_kind_panics_on_a_cardtype_move_build_never_targets() {
        build_target_kind(CardType::Government);
    }

    #[test]
    fn decision_kind_splits_build_by_target_and_leaves_every_other_move_kind_distinct() {
        assert_eq!(decision_kind(Move::Take { slot: 0 }), DecisionKind::Take);
        assert_eq!(decision_kind(Move::PopFree), DecisionKind::Pop);
        assert_eq!(decision_kind(Move::WonderStep { steps: 1 }), DecisionKind::WonderStep);
        assert_eq!(decision_kind(Move::PolPass), DecisionKind::Other);
    }

    #[test]
    fn decision_kind_reads_end_turn_as_its_own_kind_distinct_from_other() {
        assert_eq!(decision_kind(Move::EndTurn), DecisionKind::EndTurn);
        assert_ne!(DecisionKind::EndTurn, DecisionKind::Other);
    }

    #[test]
    fn probe_build_kind_is_none_for_every_non_build_shaped_move() {
        assert_eq!(probe_build_kind(Move::Take { slot: 0 }), None);
        assert_eq!(probe_build_kind(Move::EndTurn), None);
        assert_eq!(probe_build_kind(Move::PopFree), None);
    }

    #[test]
    fn probe_build_kind_reads_wonder_step_as_its_own_kind_distinct_from_every_build_target() {
        assert_eq!(probe_build_kind(Move::WonderStep { steps: 2 }), Some(BuildKind::WonderStep));
    }

    #[test]
    fn percentiles_u32_reports_min_and_max_at_the_ends_of_a_sorted_sample() {
        let s = percentiles_u32(vec![5, 1, 3, 2, 4]);
        assert!(s.contains("min=1"));
        assert!(s.contains("max=5"));
        assert!(s.contains("n=5"));
    }

    #[test]
    fn percentiles_u32_reports_n_a_for_an_empty_sample_rather_than_dividing_by_zero() {
        assert_eq!(percentiles_u32(vec![]), "n/a (no samples)");
    }

    #[test]
    fn decision_kind_counts_total_equals_the_sum_of_every_recorded_kind() {
        let mut c = DecisionKindCounts::default();
        c.record(DecisionKind::Take);
        c.record(DecisionKind::BuildFarm);
        c.record(DecisionKind::Other);
        assert_eq!(c.total(), 3);
    }

    #[test]
    fn classify_farm_illegal_reports_no_such_card_in_play_when_the_tableau_has_no_farm_tech() {
        let mut state = game::new_game(2, 1);
        // Remove every Farm-type tech from player 0's tableau so the "no
        // such card in play" branch is exercised directly.
        let farm_ids: Vec<CardId> = state.players[0].techs.of_type(CardType::Farm).map(|(id, _)| id).collect();
        for id in farm_ids {
            state.players[0].techs.remove(id);
        }
        let (reason, _multi) = classify_farm_illegal(&state, &state.players[0]);
        assert_eq!(reason, FarmIllegalReason::CardNotInPlay);
    }

    #[test]
    fn classify_farm_illegal_reports_no_free_worker_when_the_owned_farm_card_has_none_left() {
        let mut state = game::new_game(2, 1);
        state.players[0].workers_free = 0;
        // A fresh game's Agriculture is fully staffed already, so this is
        // already the true starting condition -- asserted rather than
        // assumed.
        assert!(state.players[0].techs.of_type(CardType::Farm).next().is_some());
        let (reason, _multi) = classify_farm_illegal(&state, &state.players[0]);
        assert_eq!(reason, FarmIllegalReason::FreeWorkerUnavailable);
    }

    #[test]
    fn a_truncated_2p_self_play_game_stops_at_round_4_and_records_at_least_one_decision() {
        let weights = Weights::default();
        let report = play_one(2, weights, 42);
        assert_eq!(report.games, 1);
        assert!(report.decisions > 0, "expected at least one own-turn decision point in rounds 1-3");
        assert!(report.top_kind.total() > 0);
        assert_eq!(
            report.pop_legal + report.pop_illegal,
            report.decisions,
            "every own-turn decision point must be counted exactly once as Pop legal or illegal"
        );
        assert_eq!(
            report.develop_legal + report.develop_illegal,
            report.decisions,
            "every own-turn decision point must be counted exactly once as Develop legal or illegal"
        );
        // Item 6: the rounds-1-3 ledger must be internally consistent. Every
        // recorded debit lands in exactly one round bucket, the CA/MA totals
        // never exceed the number of decision points times the row-4 take
        // cap (4 CA / 3 MA), and the per-turn denominators are bounded by
        // the per-decision ones.
        let ap = &report.action_points;
        assert_eq!(
            ap.ca_round1 + ap.ca_round2 + ap.ca_round3,
            ap.ca,
            "every CA debit must land in exactly one round bucket"
        );
        assert_eq!(
            ap.ma_round1 + ap.ma_round2 + ap.ma_round3,
            ap.ma,
            "every MA debit must land in exactly one round bucket"
        );
        assert!(
            ap.ca <= 4 * report.decisions,
            "no single decision debits more than a row-4 take (4 CA)"
        );
        assert!(
            ap.ma <= 3 * report.decisions,
            "no single decision debits more than a Culture war (3 MA)"
        );
        assert!(
            ap.ca_decisions <= report.decisions && ap.ma_decisions <= report.decisions,
            "per-decision counts cannot exceed total decisions"
        );
    }

    #[test]
    fn classify_pop_illegal_reports_round1_take_only_when_the_round_is_one() {
        let state = game::new_game(2, 1);
        assert_eq!(state.round, 1);
        assert_eq!(classify_pop_illegal(&state, &state.players[0]), PopIllegalReason::Round1TakeOnly);
    }

    #[test]
    fn classify_pop_illegal_reports_yellow_bank_empty_when_the_bank_has_no_worker() {
        let mut state = game::new_game(2, 1);
        state.round = 2;
        state.players[0].yellow_bank = 0;
        assert_eq!(classify_pop_illegal(&state, &state.players[0]), PopIllegalReason::YellowBankEmpty);
    }

    #[test]
    fn classify_pop_illegal_reports_no_civil_action_when_the_bank_has_a_worker_but_ca_is_spent() {
        let mut state = game::new_game(2, 1);
        state.round = 2;
        state.players[0].civil_actions = 0;
        state.players[0].military_actions = 0; // no spare Hammurabi conversion either
        assert!(state.players[0].yellow_bank > 0, "a fresh game's yellow bank starts nonzero");
        assert_eq!(classify_pop_illegal(&state, &state.players[0]), PopIllegalReason::CivilActionUnavailable);
    }

    #[test]
    fn classify_develop_illegal_reports_round1_take_only_when_the_round_is_one() {
        let state = game::new_game(2, 1);
        assert_eq!(state.round, 1);
        assert_eq!(classify_develop_illegal(&state, &state.players[0]).0, DevelopIllegalReason::Round1TakeOnly);
    }

    #[test]
    fn classify_develop_illegal_reports_no_eligible_card_when_hand_civil_is_empty() {
        let mut state = game::new_game(2, 1);
        state.round = 2;
        assert!(state.players[0].hand_civil.is_empty(), "a fresh game deals no civil hand before any Take");
        assert_eq!(
            classify_develop_illegal(&state, &state.players[0]).0,
            DevelopIllegalReason::NoEligibleCardInHand
        );
    }

    #[test]
    fn develop_eligible_kind_accepts_government_and_every_takes_workers_or_specialtech_type_only() {
        assert!(develop_eligible_kind(CardType::Government));
        assert!(develop_eligible_kind(CardType::Farm));
        assert!(develop_eligible_kind(CardType::SpecialTech));
        assert!(!develop_eligible_kind(CardType::Wonder));
        assert!(!develop_eligible_kind(CardType::Action));
        assert!(!develop_eligible_kind(CardType::Leader));
    }
}
