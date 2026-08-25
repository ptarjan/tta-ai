//! `behavcensus` -- a single self-play instrument covering the human-strategy
//! doc's "what does the champion actually DO" questions that `botcensus.rs`
//! (action-class RATES) does not answer: openings, wonder choice/abandon,
//! government timing, military strength by age, worker allocation by age,
//! culture/science rate by age, and final score.
//!
//! Written as one binary per the project's "several questions at once"
//! allowance, rather than one tool per question, because every one of these
//! reads off the SAME self-play loop over the SAME games -- splitting them
//! would replay the corpus N times for no new information.
//!
//! ```text
//! cargo run --profile difftest --bin behavcensus -- \
//!     --games 200 --players 2 --weights /path/to/champion_2p_snapshot.json --threads 2
//! ```
//!
//! # Method
//!
//! Every seat plays the same `weights` (a self-play mirror match, matching
//! `botcensus.rs`'s and `climb.rs`'s own convention). Age-boundary snapshots
//! (worker allocation, food/resources, strength, culture/science rate) are
//! taken from the PRE-move state at the move where `state.age_civil` is
//! observed to change -- i.e. "end of age X" is the state exactly before the
//! first move that advances play out of age X, the same "state before the
//! transition" convention `botcensus.rs`'s war-victor detection documents
//! and uses. This is a small, named approximation (the transitioning move
//! itself may already reflect a step "into" the new age) rather than an
//! exact age-boundary cutover, because the engine has no explicit
//! "age just ended" event to hook.
//!
//! Score decomposition by source (wonders vs culture buildings vs leaders
//! etc.) is NOT reported: `game::scores` returns `PlayerState::culture`, a
//! single accumulating stock with no source-tagged ledger anywhere in the
//! engine (`finish_game` folds the one end-of-game bonus straight into
//! `culture` too) -- there is nothing to decompose without inventing new
//! instrumentation, which the task instructions say not to do for this pass.

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Search, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::cards::Special;
use tta::effects;
use tta::game::{self, MOVE_CAP};
use tta::legal;
use tta::moves::Move;
use tta::state::{ChoiceKind, Pending};
use tta::{Age, CardId, CardType};

// ---------------------------------------------------------------------
// Wonder fate classification
// ---------------------------------------------------------------------
//
// A behaviour census once read "wonders started" (distinct `CardId`s ever
// seen in a player's `wonder` slot) minus "wonders completed" and called the
// gap "abandoned" -- but the base game has no such verb. Every distinct
// wonder that ever occupied the slot leaves it for exactly one of the
// reasons below; this enum makes that closed set explicit instead of
// inferring it from a name-set difference.

/// The fate of one wonder that occupied a player's `wonder` slot at some
/// point in a game. Exactly one variant applies to every distinct `CardId`
/// a player-game's `wonder` slot ever held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum WonderFate {
    /// Paid off in full; moved to `completed_wonders` (`apply::do_wonder_step`).
    Completed,
    /// A rival's `Infiltrate` aggression (`Special::RemoveFromGame`) chose
    /// this player's wonder over their leader, or they had no leader to
    /// choose between (`interact.rs`'s `QueueItem::Infiltrate` handler /
    /// `ChoiceKind::Infiltrate`): the wonder is discarded and `wonder_steps`
    /// zeroed. Detected structurally by `infiltrate_candidate_victim`, not
    /// by pattern-matching `Move::Choose` -- see its doc comment for why a
    /// single `Move::Choose` match is structurally blind to the common,
    /// auto-resolved, no-leader case.
    DestroyedByInfiltrate,
    /// Cleared at an age boundary because it was older than the age that
    /// just ended (`game::antiquate`; RULES_SPEC.md line 252/299: "an
    /// UNFINISHED wonder of an age older than the age that just ended is
    /// removed from play"). Legal per the base rulebook, §12.2.
    DestroyedByAntiquation,
    /// Left the slot for neither reason above. The engine has exactly three
    /// sites that ever clear `.wonder` -- completion (`apply.rs`),
    /// antiquation (`game::antiquate`), and Infiltrate (`interact.rs`'s
    /// `QueueItem::Infiltrate` handler) -- and this census now recognizes
    /// all three, including Infiltrate's auto-resolved shape (see
    /// `infiltrate_candidate_victim`'s doc comment). A non-zero count here
    /// is therefore NOT a classification gap: it means the engine cleared
    /// `.wonder` from a fourth site the rulebook does not sanction, and is a
    /// real bug to chase, not a census TODO.
    DestroyedUnexplained,
    /// Still occupying the slot when the game ended (unfinished, never
    /// destroyed).
    StillInProgress,
}

/// Tallies of [`WonderFate`] across many player-games. A struct of named
/// counters rather than a `HashMap<WonderFate, u64>` so every fate has a
/// fixed, always-present slot -- a fate nobody hit prints as `0`, not as a
/// missing key.
#[derive(Default, Clone, Copy)]
struct WonderFateCounts {
    completed: u64,
    infiltrated: u64,
    antiquated: u64,
    unexplained: u64,
    still_in_progress: u64,
}

impl WonderFateCounts {
    fn record(&mut self, fate: WonderFate) {
        match fate {
            WonderFate::Completed => self.completed += 1,
            WonderFate::DestroyedByInfiltrate => self.infiltrated += 1,
            WonderFate::DestroyedByAntiquation => self.antiquated += 1,
            WonderFate::DestroyedUnexplained => self.unexplained += 1,
            WonderFate::StillInProgress => self.still_in_progress += 1,
        }
    }

    fn total(&self) -> u64 {
        self.completed + self.infiltrated + self.antiquated + self.unexplained + self.still_in_progress
    }

    fn merge(&mut self, other: WonderFateCounts) {
        self.completed += other.completed;
        self.infiltrated += other.infiltrated;
        self.antiquated += other.antiquated;
        self.unexplained += other.unexplained;
        self.still_in_progress += other.still_in_progress;
    }
}

// ---------------------------------------------------------------------
// Wonder-tempo EARLY/LATE grouping
// ---------------------------------------------------------------------
//
// analysis/wonder_tempo_2026-08-24.txt measured that a player-game whose
// first wonder-stage build lands at round 10+ wins well below the self-play
// null (24.6% vs 33.3% at 3p, p=0.0171), while the bulk of the population
// starts by round 5. This section answers the follow-up question the
// analysis could not: what did the LATE group's civil actions go toward
// instead, and how does its board differ from the EARLY group's by the
// point (round 6) where the gap is already visible.

/// The two groups the wonder-tempo comparison below is over. A player-game
/// whose first wonder-stage round falls in 6..=9 is in NEITHER group (see
/// `wonder_tempo_group`) -- this is the boundary the analysis used, not an
/// exhaustive split of the population.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WonderTempoGroup {
    /// First wonder-stage build by round 5.
    Early,
    /// First wonder-stage build at round 10 or later, or never built at all.
    Late,
}

/// Classifies one player-game's `PlayerTrack::first_wonder_round` into a
/// [`WonderTempoGroup`], or `None` for the excluded middle band (round
/// 6-9) -- pulled out of `play_one`'s end-of-game loop so the boundary is
/// defined in exactly one place.
fn wonder_tempo_group(first_wonder_round: Option<u16>) -> Option<WonderTempoGroup> {
    match first_wonder_round {
        Some(r) if r <= 5 => Some(WonderTempoGroup::Early),
        Some(r) if r >= 10 => Some(WonderTempoGroup::Late),
        Some(_) => None,
        // Never built a wonder stage at all: grouped with LATE, matching
        // analysis/wonder_tempo_2026-08-24.txt's own "rounds 10+" win-rate
        // bucket, which folds in the "never built" player-games rather than
        // giving them a third bucket.
        None => Some(WonderTempoGroup::Late),
    }
}

/// A civil-action-consuming move, bucketed into the coarse kind the
/// round-3-9 cross-tab reports spend by. Coarser than `Move` itself (a
/// single named enum per task-requested bucket) rather than keying a map by
/// `Move` directly, matching this file's own preference for named buckets
/// over reusing an engine type as a report key (see `opening_build_kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CivilMoveKind {
    Take,
    Build,
    Develop,
    Pop,
    Leader,
    ActionCard,
    WonderStep,
    /// Everything else that can still spend a civil action (Revolution,
    /// Upgrade, BachTheater, ...) or spends none at all (a response move
    /// like `Choose`/`Defend`, which only ever reaches this classifier when
    /// `ca_spent > 0` happens to be true for it too).
    Other,
}

fn civil_move_kind(mv: Move) -> CivilMoveKind {
    match mv {
        Move::Take { .. } => CivilMoveKind::Take,
        Move::Build { .. } => CivilMoveKind::Build,
        Move::Develop { .. } => CivilMoveKind::Develop,
        Move::Pop { .. } | Move::PopFree => CivilMoveKind::Pop,
        Move::PlayLeader { .. } => CivilMoveKind::Leader,
        Move::PlayAction { .. } => CivilMoveKind::ActionCard,
        Move::WonderStep { .. } => CivilMoveKind::WonderStep,
        Move::Upgrade { .. }
        | Move::Revolution { .. }
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
        | Move::Resign => CivilMoveKind::Other,
    }
}

/// Tallies of civil-action SPEND (not move count -- `record`'s `spent` is
/// the number of civil actions the move consumed) by [`CivilMoveKind`]. A
/// struct of named counters for the same reason [`WonderFateCounts`] is one
/// rather than a `HashMap<CivilMoveKind, u64>`: a kind nobody spent on
/// prints as `0`, not as a missing key.
#[derive(Default, Clone, Copy)]
struct CivilSpendCounts {
    take: u64,
    build: u64,
    develop: u64,
    pop: u64,
    leader: u64,
    action_card: u64,
    wonder_step: u64,
    other: u64,
}

impl CivilSpendCounts {
    fn record(&mut self, kind: CivilMoveKind, spent: u64) {
        match kind {
            CivilMoveKind::Take => self.take += spent,
            CivilMoveKind::Build => self.build += spent,
            CivilMoveKind::Develop => self.develop += spent,
            CivilMoveKind::Pop => self.pop += spent,
            CivilMoveKind::Leader => self.leader += spent,
            CivilMoveKind::ActionCard => self.action_card += spent,
            CivilMoveKind::WonderStep => self.wonder_step += spent,
            CivilMoveKind::Other => self.other += spent,
        }
    }

    fn total(&self) -> u64 {
        self.take + self.build + self.develop + self.pop + self.leader + self.action_card + self.wonder_step + self.other
    }

    fn merge(&mut self, other: CivilSpendCounts) {
        self.take += other.take;
        self.build += other.build;
        self.develop += other.develop;
        self.pop += other.pop;
        self.leader += other.leader;
        self.action_card += other.action_card;
        self.wonder_step += other.wonder_step;
        self.other += other.other;
    }
}

// ---------------------------------------------------------------------
// Card fate: are the LATE group's extra taken civil cards WASTED, or a
// real investment that pays off later?
// ---------------------------------------------------------------------
//
// analysis/wonder_tempo_2026-08-24.txt's follow-up: the LATE group's
// take-share of civil-action spend is higher than EARLY's in 5 of 7
// rounds (see the "Wonder tempo" report section below), and LATE ends
// round 6 holding more civil cards. This section tracks every civil card
// a `Move::Take` ever puts into a player's `hand_civil` to its eventual
// resolution -- played, discarded at an age transition, or still sitting
// in hand at game end -- so that question is answerable directly instead
// of guessed at from the round-6 hand-size snapshot alone.
//
// Card IDENTITY (not just counts) is tracked: `PlayerTrack::taken_rounds`
// is keyed by `CardId`, one FIFO queue of "round taken" per distinct card
// a player has held. A queue, not a single value, because a player can
// hold more than one physical copy of the same-named card in hand at
// once, and `CardList::remove_first` (every production site that takes a
// card OUT of `hand_civil` -- `apply::h_play_leader`, the `Develop`
// handler, `apply::h_revolution`, `apply::h_play_action`) removes the
// EARLIEST matching instance, exactly the semantics a FIFO queue gives
// `pop_front`.
//
// A wonder taken by `Move::Take` never reaches this tracking at all: per
// `apply::take_card_impl`, a wonder card goes straight into the `.wonder`
// slot, never through `hand_civil` -- the same fact `WonderFate`'s own
// section above depends on.
//
// Item 4 of the requested measurements ("cards lost to an age transition
// OR hand-limit discard") only has one live half: age-transition
// antiquation (`game::antiquate_hands`, RULES_SPEC.md line 252/299) is a
// real discard event and is tracked below via a pre/post `hand_civil`
// diff on the move that advances `state.age_civil` (mirroring `WonderFate
// ::DestroyedByAntiquation`'s own detection). A civil HAND-LIMIT discard
// event does not exist in this engine: `civil_hand_limit` only blocks an
// illegal `Move::Take` (see `game::force_civil_age_at_least`'s doc
// comment) -- it never forces a player to discard down to it on its own.
// That half of item 4 is therefore not counted, because there is nothing
// to count; see cardfate_notes.txt for this stated explicitly rather than
// left implicit.

/// The three fates a distinct taken civil card (one `taken_rounds` queue
/// entry) resolves to by game end. Every entry resolves to exactly one --
/// `CardFateCounts::taken` is the sum of the other three, checked by
/// construction (each entry is pushed once by a `Take` and popped by
/// exactly one of the two event sites below, or never popped at all).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardFate {
    /// Left `hand_civil` via `Develop`/`PlayLeader`/`Revolution`/
    /// `PlayAction` -- see [`played_civil_card`].
    Played,
    /// Culled by age-transition antiquation before ever being played.
    Antiquated,
    /// Neither of the above by game end.
    StillInHand,
}

/// Tallies of [`CardFate`] across many player-games. A struct of named
/// counters for the same reason [`WonderFateCounts`]/[`CivilSpendCounts`]
/// are: a fate nobody hit prints as `0`, not as a missing key.
/// `record`'s `n` parameter (rather than always-1) matches
/// [`CivilSpendCounts::record`]'s shape: the caller already knows how many
/// of a player-game's taken cards resolved to a given fate and records the
/// whole count in one call.
#[derive(Default, Clone, Copy)]
struct CardFateCounts {
    taken: u64,
    played: u64,
    antiquated: u64,
    still_in_hand: u64,
}

impl CardFateCounts {
    fn record(&mut self, fate: CardFate, n: u64) {
        self.taken += n;
        match fate {
            CardFate::Played => self.played += n,
            CardFate::Antiquated => self.antiquated += n,
            CardFate::StillInHand => self.still_in_hand += n,
        }
    }

    fn total(&self) -> u64 {
        self.taken
    }

    fn merge(&mut self, other: CardFateCounts) {
        self.taken += other.taken;
        self.played += other.played;
        self.antiquated += other.antiquated;
        self.still_in_hand += other.still_in_hand;
    }
}

/// The `CardId` a civil card was played AS, if `mv` is one of the four
/// sites that ever call `hand_civil.remove_first` in production code
/// (`apply::h_play_leader`, the `Develop` handler, `apply::h_revolution`,
/// `apply::h_play_action` -- confirmed by grep, there are no others).
/// `Move::Build`/`Move::Upgrade` do NOT touch `hand_civil` at all: both
/// operate on a card already sitting in `PlayerState::techs` (`apply::
/// do_build`'s own `p.techs.get_mut(id).expect("...must already be
/// developed...")`), so a built/upgraded card's hand-departure already
/// happened, earlier, at whichever `Develop` move put it in the tableau --
/// this function's "played" is that Develop moment, not the later build.
/// Exhaustive over every `Move` variant, matching this file's own
/// `civil_move_kind`, so a future `Move` variant that also drains
/// `hand_civil` cannot silently fall through unclassified.
fn played_civil_card(mv: Move) -> Option<CardId> {
    match mv {
        Move::Develop { card, .. } => Some(card),
        Move::PlayLeader { card } => Some(card),
        Move::Revolution { card } => Some(card),
        Move::PlayAction { card } => Some(card),
        Move::Take { .. }
        | Move::Build { .. }
        | Move::Upgrade { .. }
        | Move::WonderStep { .. }
        | Move::Pop { .. }
        | Move::PopFree
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

/// Cards present in `pre` but not matched one-for-one in `post` -- a plain
/// multiset difference (not a set difference: `hand_civil` can hold more
/// than one copy of the same `CardId`, so a naive "seen in post" boolean
/// check would under-count a hand that lost one of two identical copies).
/// `O(pre.len() * post.len())`, fine at real `hand_civil` sizes (a handful
/// of cards, gated by `MAX_HAND`), called at most once per game per player
/// (only on the single move that advances `state.age_civil`).
fn hand_multiset_diff(pre: &[CardId], post: &[CardId]) -> Vec<CardId> {
    let mut remaining: Vec<CardId> = post.to_vec();
    let mut removed = Vec::new();
    for &card in pre {
        match remaining.iter().position(|&c| c == card) {
            Some(pos) => {
                remaining.swap_remove(pos);
            }
            None => removed.push(card),
        }
    }
    removed
}

// ---------------------------------------------------------------------
// Card fate follow-up: WHY does a never-played card rot -- was it ever
// legal to play, or did it lose the civil-action auction every turn?
// ---------------------------------------------------------------------
//
// analysis/card_fate_human_2026-08-24.txt left this open: 39% of every
// civil card the bot takes is never played, and that could mean (a) the
// card was never affordable/legal again after the take (a bad take), or
// (b) it WAS legal on some later turn but the bot always spent its civil
// actions on something else. This section answers that by checking, at
// every one of the acting player's own decision points, whether each card
// already sitting in `hand_civil` had a matching move in `legal::
// legal_moves` -- the engine's own move generator, the same one `Seat::
// pick` already calls to choose `mv` (see `legal.rs`'s "single source of
// truth" doc comment) -- rather than re-deriving affordability from
// `costs.rs`, which could silently drift from what the bot was actually
// allowed to do.

/// One outstanding (not yet played or antiquated) copy of a taken civil
/// card. Bundles "round taken" with "how many decision points it has been
/// legal to play since" into ONE queue entry, rather than a second
/// `VecDeque` kept in step with `taken_rounds`'s by position -- the two
/// numbers must pop together off the same queue entry, and a pair of
/// parallel collections could let them drift out of sync.
#[derive(Clone, Copy, Debug)]
struct TakenCard {
    taken_round: u16,
    /// Count of this player's own decision points, since this card was
    /// taken, at which Develop/PlayLeader/Revolution/PlayAction of this
    /// exact `CardId` appeared in `legal::legal_moves` -- i.e. the CA
    /// auction was there to be won, whether or not the bot won it.
    playable_turns: u32,
    /// Count of this player's own decision points, since this card was
    /// taken, at which NO civil card in hand was legal and the player had
    /// NO civil action left to spend -- the hand may well have been
    /// affordable, there was simply nothing left to pay the action with.
    /// This is an action-budget miss, not a production shortfall, and the
    /// two have opposite fixes.
    ///
    /// Slight OVER-count: Hammurabi can convert a military action into a
    /// civil one at `civil_actions == 0`
    /// (`costs::hammurabi_conversion_available`), which is `pub(crate)` and
    /// so unreachable from a bin target. One leader, so the leak is small.
    blocked_no_civil_action: u32,
    /// Count of this player's own decision points, since this card was
    /// taken, at which NO civil card in hand was legal even though the
    /// player still had a civil action to spend -- a true production
    /// shortfall: the action was there and nothing in hand could be paid
    /// for.
    blocked_nothing_affordable: u32,
    /// Count of this player's own decision points, since this card was
    /// taken, at which this card was NOT legal even though some OTHER
    /// civil card in hand WAS -- a card-selection defect: this specific
    /// card lost the CA auction to something that was actually buildable.
    blocked_something_else_developable: u32,
}

// ---------------------------------------------------------------------
// Per-age snapshot bucket
// ---------------------------------------------------------------------

/// One end-of-age (or end-of-game, for the `IV` slot) observation for one
/// player in one game.
#[derive(Clone, Copy)]
struct AgeSample {
    farm_workers: u32,
    mine_workers: u32,
    lab_workers: u32,
    food: u32,
    resources: u32,
    strength: i32,
    culture_rate: i32,
    science_rate: i32,
    /// Fields below are unused by the per-age table this struct was built
    /// for -- they exist so the round-6 wonder-tempo snapshot (`Report::
    /// round6_early`/`round6_late`) can reuse the pre-move sample this file
    /// already takes on EVERY move (`play_one`'s `pre_snapshots`), instead
    /// of calling `effects::state_stats` a second time per move. `food`/
    /// `resources` above are STOCK; `food_rate`/`resource_rate` here are
    /// PRODUCTION (`effects::Stats::food`/`resources` -- the two structs
    /// name their fields identically but mean different things).
    culture_stock: u32,
    food_rate: i32,
    resource_rate: i32,
    hand_civil: u32,
    buildings: u32,
}

fn age_index(age: Age) -> usize {
    match age {
        Age::A => 0,
        Age::I => 1,
        Age::II => 2,
        Age::III => 3,
        Age::IV => 4,
    }
}

fn age_label(i: usize) -> &'static str {
    match i {
        0 => "end of Age A",
        1 => "end of Age I",
        2 => "end of Age II",
        3 => "end of Age III",
        4 => "end of Age IV (game end)",
        _ => "?",
    }
}

fn sample_player(state: &tta::GameState, idx: u8) -> AgeSample {
    let p = &state.players[idx as usize];
    let s = effects::state_stats(state, p);
    AgeSample {
        farm_workers: p.techs.of_type(CardType::Farm).map(|(_, slot)| slot.workers as u32).sum(),
        mine_workers: p.techs.of_type(CardType::Mine).map(|(_, slot)| slot.workers as u32).sum(),
        lab_workers: p.techs.of_type(CardType::Lab).map(|(_, slot)| slot.workers as u32).sum(),
        food: p.food as u32,
        resources: p.resources as u32,
        strength: s.strength,
        culture_rate: s.culture,
        science_rate: s.science,
        culture_stock: p.culture as u32,
        food_rate: s.food,
        resource_rate: s.resources,
        hand_civil: p.hand_civil.len() as u32,
        // "Buildings on board": the 7 urban/farm/mine CardTypes, counted as
        // distinct developed CardIds rather than workers (unlike
        // `farm_workers`/`mine_workers` above) -- mirrors `opening_build_
        // kind`'s own building/military split, just summed instead of named.
        buildings: (p.techs.of_type(CardType::Farm).count()
            + p.techs.of_type(CardType::Mine).count()
            + p.techs.of_type(CardType::Temple).count()
            + p.techs.of_type(CardType::Lab).count()
            + p.techs.of_type(CardType::Library).count()
            + p.techs.of_type(CardType::Arena).count()
            + p.techs.of_type(CardType::Theater).count()) as u32,
    }
}

// ---------------------------------------------------------------------
// Percentile helper -- report distributions, not means alone (project rule)
// ---------------------------------------------------------------------

fn percentiles_i32(mut v: Vec<i32>) -> String {
    if v.is_empty() {
        return "n/a (no samples)".to_string();
    }
    v.sort_unstable();
    let at = |p: f64| -> i32 {
        let i = ((v.len() - 1) as f64 * p).round() as usize;
        v[i]
    };
    let mean: f64 = v.iter().map(|&x| x as f64).sum::<f64>() / v.len() as f64;
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

fn percentiles_u32(v: Vec<u32>) -> String {
    percentiles_i32(v.into_iter().map(|x| x as i32).collect())
}

// ---------------------------------------------------------------------
// Per-run tally
// ---------------------------------------------------------------------

/// One player-round's contribution to the "Worker allocation curve" --
/// summed here, divided by `n` in `print_report`. Sampled at the exact
/// same instant as `Report::production_by_round` (see that field's doc);
/// `bin/humanopenings.rs` accumulates the identically-named/-shaped
/// quantity so the two curves are directly diffable line-for-line.
#[derive(Default, Clone, Copy)]
struct AllocAccum {
    farm_workers: u64,
    mine_workers: u64,
    urban_workers: u64,
    mil_workers: u64,
    free_workers: u64,
    staffed_workers: u64,
    best_farm_sum: u64,
    best_mine_sum: u64,
    n: u64,
}

// ---------------------------------------------------------------------
// Tech acquisition: where does a production tech die in the pipeline?
// ---------------------------------------------------------------------
//
// analysis/worker_allocation_3p_2026-08-24.txt measured the bot's best-owned
// MINE tech level flat at the starting Bronze for all 21 sampled rounds, and
// a follow-up recon pass established the leaf evaluation prices `BestMine`
// positively -- the champion WANTS a better mine and never gets one. This
// section instruments the four-stage pipeline a production tech (or any
// other developable civil card) must survive to reach a worker: appears in
// the card row (SEEN) -> taken into hand (TAKEN) -> tech cost paid, card
// enters the tableau via `Move::Develop` (BUILT) -> a worker is placed on it
// via `Move::Build`/`Move::Pop`/`Move::PopFree` (STAFFED).
//
// Reuses the SAME two hooks the "Worker allocation curve" section above
// already taps at the SAME instant (the `prev_actor` turn-start sample, and
// the `p.techs.iter()` walk already inside that block) rather than adding a
// new sample point: SEEN reads `state.card_row` at that instant; BUILT/
// STAFFED read tech/worker membership off the exact same `p.techs.iter()`
// loop that already computes `farm_workers`/`mine_workers`/etc. there (BUILT
// = card present in `techs` at a turn-start sample; STAFFED = present with
// `workers > 0`). TAKEN is the one event-exact signal, read off the
// `taken_card` this file already computes pre-move for every decision (see
// the "openings" block below it) -- no new hook, no new sample instant.
// `Move::Upgrade` moves a worker between two cards that are BOTH already
// built (`apply::do_upgrade`'s own `.expect("hi not in tableau")`), so it
// can never be how a card first reaches BUILT and is not tracked here.

/// The coarse card-type buckets this section reports on: production
/// (Farm/Mine, the two types under direct suspicion), the four urban types
/// the task named explicitly (Lab/Temple/Arena/Library -- Theater is NOT in
/// that list and is deliberately excluded here; it still contributes to the
/// Worker allocation curve's `urban_workers` aggregate above), and military
/// units folded into one bucket (matching `opening_build_kind`'s own
/// "Military" bucket). A closed, named set rather than a filter over
/// `CardType` at print time, so a type nobody hit still prints an explicit
/// `0` row -- same "no missing keys" rule `WonderFateCounts` uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TechKind {
    Farm,
    Mine,
    Lab,
    Temple,
    Arena,
    Library,
    Military,
}

const ALL_TECH_KINDS: [TechKind; 7] = [
    TechKind::Farm,
    TechKind::Mine,
    TechKind::Lab,
    TechKind::Temple,
    TechKind::Arena,
    TechKind::Library,
    TechKind::Military,
];

fn tech_kind_label(k: TechKind) -> &'static str {
    match k {
        TechKind::Farm => "Farm",
        TechKind::Mine => "Mine",
        TechKind::Lab => "Lab",
        TechKind::Temple => "Temple",
        TechKind::Arena => "Arena",
        TechKind::Library => "Library",
        TechKind::Military => "Military",
    }
}

/// Maps an engine `CardType` to this section's [`TechKind`] bucket, or
/// `None` for a type this section does not report on (Theater and every
/// non-developable/non-worker type). Exhaustive, no wildcard arm --
/// `wildcard_enum_match_arm` is denied repo-wide.
fn tech_kind(k: CardType) -> Option<TechKind> {
    match k {
        CardType::Farm => Some(TechKind::Farm),
        CardType::Mine => Some(TechKind::Mine),
        CardType::Lab => Some(TechKind::Lab),
        CardType::Temple => Some(TechKind::Temple),
        CardType::Arena => Some(TechKind::Arena),
        CardType::Library => Some(TechKind::Library),
        CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => Some(TechKind::Military),
        CardType::Theater
        | CardType::Government
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
        | CardType::Event => None,
    }
}

/// One [`TechKind`]'s tally across the four pipeline stages, summed over
/// many player-games (divided by `n` at print time). A monotone-decreasing
/// staircase (seen the most, staffed the least) is expected but not
/// enforced by construction: SEEN and STAFFED are turn-start snapshots (see
/// this section's own doc comment), so a card built and staffed, or
/// destroyed, entirely within one of a player's own turns is not guaranteed
/// to land on the sample instant that would count it -- a small, named
/// approximation, not a correctness bug.
#[derive(Default, Clone, Copy)]
struct StageCounts {
    seen: u64,
    taken: u64,
    built: u64,
    staffed: u64,
}

/// Farm/Mine age-tier breakdown for TAKEN and BUILT only (the two stages the
/// task asked to break down by tier) -- index 0..=3 is Age A/I/II/III, the
/// only ages Farm/Mine cards occupy in `data/cards_civil.json` (confirmed by
/// inspection: Agriculture/Irrigation/Selective Breeding/Mechanized
/// Agriculture and Bronze/Iron/Coal/Oil are the only Farm/Mine cards, one
/// per age A through III, none in age IV).
#[derive(Default, Clone, Copy)]
struct TierCounts {
    taken: [u64; 4],
    built: [u64; 4],
}

#[derive(Default)]
struct Report {
    games: u64,

    // ---- openings (one sample per player per game) ----
    first_take_card: HashMap<&'static str, u64>,
    first_develop_card: HashMap<&'static str, u64>,
    first_develop_round: Vec<i32>,
    /// Round of first `WonderStep` move -> (wins, total), per player-game
    /// that ever built one; same (wins, total) shape as `opening_first_take`
    /// / `opening_first_build_kind` below, so a win-rate prints next to
    /// every round the same way theirs does. Player-games with an
    /// unresolvable outcome (see `is_winner`) are excluded, matching those
    /// two maps.
    first_wonder_round: HashMap<i32, (u64, u64)>,
    /// (wins, total) for player-games that never built a wonder stage at
    /// all -- its own labelled bucket, never folded into `first_wonder_round`.
    first_wonder_never_built: (u64, u64),
    n_player_games_no_wonder_by_round4: u64,
    n_player_games: u64,

    // ---- production curve (one sample per player-round, taken at the
    // START of that player's turn -- before any move of theirs is applied
    // -- via `economy::production_this_turn`; see `play_one`'s `prev_actor`
    // sampling). (sum food, sum resources, n) keyed by round, so
    // `print_report`'s "Production curve" section can print a mean per
    // round directly comparable against `bin/humanopenings.rs`'s human
    // curve of the same shape.
    production_by_round: HashMap<u16, (u64, u64, u64)>,

    // ---- worker allocation curve (same sample instant as
    // `production_by_round` above, see its doc and `AllocAccum`'s) ----
    alloc_by_round: HashMap<u16, AllocAccum>,

    // ---- tech acquisition: seen/taken/built/staffed per `TechKind`, one
    // sample per player-game (see the "Tech acquisition" section above for
    // the pipeline definition and the hooks reused to measure it). Denominator
    // for the printed per-player-game rate is `n_player_games` below -- every
    // player-game contributes exactly one sample to every field here, so no
    // separate counter is kept.
    tech_acq: HashMap<TechKind, StageCounts>,
    farm_tier: TierCounts,
    mine_tier: TierCounts,

    // ---- opening, human-comparable schema (`bin/humanopenings.rs`'s
    // schema -- see `PlayerTrack`'s own doc). Each map's value is (wins,
    // total) so a win-rate can be printed next to every count, matching
    // `docs/OPENINGS.txt`'s own requirement that a rate always carry its
    // sample size.
    opening_first_take: HashMap<&'static str, (u64, u64)>,
    opening_first_build_kind: HashMap<&'static str, (u64, u64)>,
    opening_leader_r3: HashMap<bool, (u64, u64)>,
    opening_pop_r3: HashMap<bool, (u64, u64)>,
    opening_ca_unused: HashMap<i32, u64>,
    /// CA price paid for the first Age-A take, so the bot's price
    /// distribution can be compared against `bin/humanopenings.rs`'s. The bot
    /// prices a civil action negatively, so it can pick a PRICE rather than a
    /// card; without this the overpaying claim is unmeasurable.
    opening_first_take_cost: HashMap<i32, u64>,
    /// Player-games excluded from every win-rate map above because this
    /// game hit `MOVE_CAP` (`play_one`'s `cap_hit`) rather than reaching a
    /// real `state.game_over` -- there is no winner to attribute, matching
    /// how `bin/humanopenings.rs` reports `"unknown"` rather than guessing.
    opening_outcome_unknown: u64,

    // ---- wonders ----
    wonders_completed_per_playergame: Vec<i32>,
    wonders_started_per_playergame: Vec<i32>,
    wonders_abandoned_per_playergame: Vec<i32>,
    wonder_completed_name_counts: HashMap<&'static str, u64>,
    wonder_fate: WonderFateCounts,
    /// Distinct wonders whose fate could not be resolved to exactly one
    /// [`WonderFate`] (should stay 0; see `play_one`'s consistency check).
    n_wonder_fate_mismatches: u64,

    // ---- government ----
    final_government: HashMap<&'static str, u64>,
    gov_change_count_per_playergame: Vec<i32>,
    first_gov_change_round: Vec<i32>, // only for player-games with >=1 change
    n_stayed_despotism: u64,

    // ---- military ----
    wars_declared_per_playergame: Vec<i32>,
    aggressions_played_per_playergame: Vec<i32>,
    n_weakest_at_end_age2: u64, // player-games where this player had strictly the min strength among all players at end of Age II
    n_playergames_with_age2_sample: u64,

    // ---- economy / culture / science by age ----
    age_samples: [Vec<AgeSample>; 5],

    // ---- score ----
    final_score: Vec<i32>,

    // ---- wonder tempo: civil-action spend by move kind, rounds 3-9,
    // EARLY vs LATE (see the "Wonder-tempo EARLY/LATE grouping" section
    // above and analysis/wonder_tempo_2026-08-24.txt). Index 0..7 of each
    // array is round 3..9. A player-game in the excluded round-6-9 middle
    // band (`wonder_tempo_group` returns `None`) contributes to neither.
    civil_spend_early: [CivilSpendCounts; 7],
    civil_spend_late: [CivilSpendCounts; 7],
    n_player_games_early: u64,
    n_player_games_late: u64,
    // End-of-round-6 board state, same EARLY/LATE grouping as above -- one
    // sample per player-game that reached round 6 (a game that ends before
    // round 6 contributes to neither Vec, same "no fabricated sample" rule
    // as the rest of this file).
    round6_early: Vec<AgeSample>,
    round6_late: Vec<AgeSample>,

    // ---- card fate: are LATE's extra taken civil cards wasted, or a real
    // investment that pays off later? Same EARLY/LATE grouping as the
    // wonder-tempo fields above (see the "Card fate" section for the
    // fate classification and its `hand_civil.remove_first`/antiquation
    // detection).
    card_fate_early: CardFateCounts,
    card_fate_late: CardFateCounts,
    /// A `Move::Develop`/`PlayLeader`/`Revolution`/`PlayAction`, or an
    /// antiquation cull, that could not be matched back to a `taken_rounds`
    /// entry (should stay 0 -- see `play_one`'s card-fate blocks; the same
    /// "a nonzero count here is a real bug, not a census gap" reasoning as
    /// `n_wonder_fate_mismatches`).
    n_card_fate_mismatches: u64,
    /// Civil cards taken over the whole game, one sample per player-game.
    cards_taken_per_playergame_early: Vec<i32>,
    cards_taken_per_playergame_late: Vec<i32>,
    /// Civil cards still sitting in `hand_civil` at game end, one sample
    /// per player-game (a player-game that took 0 civil cards still
    /// contributes a `0` -- same "no fabricated sample, but no silent
    /// exclusion of a real zero either" rule the rest of this file uses).
    cards_still_in_hand_per_playergame_early: Vec<i32>,
    cards_still_in_hand_per_playergame_late: Vec<i32>,
    /// Rounds a taken card sat in hand before being played -- one sample
    /// per PLAYED card (not per player-game), the played population of
    /// item 5's dwell measurement.
    played_dwell_early: Vec<i32>,
    played_dwell_late: Vec<i32>,
    /// The same dwell measurement for cards NEVER played by game end --
    /// still in hand (censored at the final round) or antiquated (censored
    /// at the round it was culled) -- kept as a SEPARATE population from
    /// `played_dwell_*` above, never blended with it, because "rounds held
    /// so far" and "rounds held before being played" answer different
    /// questions.
    censored_dwell_early: Vec<i32>,
    censored_dwell_late: Vec<i32>,
    /// `TakenCard::playable_turns` for PLAYED cards -- the control
    /// population for the never-played question below (see the "Card fate
    /// follow-up" section above `TakenCard`'s own definition).
    playable_turns_played_early: Vec<u32>,
    playable_turns_played_late: Vec<u32>,
    /// The same, for cards that were NEVER played (antiquated or still in
    /// hand at game end, blended exactly as `censored_dwell_*` already
    /// blends them). Zero here means the take was never legal again by
    /// game end; nonzero means it lost the CA auction every turn it was
    /// legal, until it was culled or the game ended.
    playable_turns_never_played_early: Vec<u32>,
    playable_turns_never_played_late: Vec<u32>,
    /// One `(blocked_no_civil_action, blocked_nothing_affordable,
    /// blocked_something_else_
    /// developable)` pair per PLAYED card -- the control population for the
    /// poverty/selection-loss follow-up (see `TakenCard`'s own doc
    /// comments for what each half of the pair counts).
    blocked_played_early: Vec<(u32, u32, u32)>,
    blocked_played_late: Vec<(u32, u32, u32)>,
    /// The same pair, restricted to NEVER-played cards that also scored
    /// zero `playable_turns` -- the population the poverty/selection-loss
    /// question is actually about: a card that was legal at least once
    /// already has its story told by `playable_turns_never_played_*`.
    blocked_never_played_zero_early: Vec<(u32, u32, u32)>,
    blocked_never_played_zero_late: Vec<(u32, u32, u32)>,
}

impl Report {
    fn merge(&mut self, mut other: Report) {
        self.games += other.games;
        merge_map(&mut self.first_take_card, other.first_take_card);
        merge_map(&mut self.first_develop_card, other.first_develop_card);
        self.first_develop_round.extend(other.first_develop_round);
        merge_pair_map(&mut self.first_wonder_round, other.first_wonder_round);
        self.first_wonder_never_built.0 += other.first_wonder_never_built.0;
        self.first_wonder_never_built.1 += other.first_wonder_never_built.1;
        self.n_player_games_no_wonder_by_round4 += other.n_player_games_no_wonder_by_round4;
        self.n_player_games += other.n_player_games;
        merge_triple_map(&mut self.production_by_round, other.production_by_round);
        merge_alloc_map(&mut self.alloc_by_round, other.alloc_by_round);
        merge_techacq_map(&mut self.tech_acq, other.tech_acq);
        merge_tier_counts(&mut self.farm_tier, other.farm_tier);
        merge_tier_counts(&mut self.mine_tier, other.mine_tier);

        merge_pair_map(&mut self.opening_first_take, other.opening_first_take);
        merge_pair_map(&mut self.opening_first_build_kind, other.opening_first_build_kind);
        merge_pair_map(&mut self.opening_leader_r3, other.opening_leader_r3);
        merge_pair_map(&mut self.opening_pop_r3, other.opening_pop_r3);
        merge_count_map(&mut self.opening_ca_unused, other.opening_ca_unused);
        merge_count_map(&mut self.opening_first_take_cost, other.opening_first_take_cost);
        self.opening_outcome_unknown += other.opening_outcome_unknown;

        self.wonders_completed_per_playergame.extend(other.wonders_completed_per_playergame);
        self.wonders_started_per_playergame.extend(other.wonders_started_per_playergame);
        self.wonders_abandoned_per_playergame.extend(other.wonders_abandoned_per_playergame);
        merge_map(&mut self.wonder_completed_name_counts, other.wonder_completed_name_counts);
        self.wonder_fate.merge(other.wonder_fate);
        self.n_wonder_fate_mismatches += other.n_wonder_fate_mismatches;

        merge_map(&mut self.final_government, other.final_government);
        self.gov_change_count_per_playergame.extend(other.gov_change_count_per_playergame);
        self.first_gov_change_round.extend(other.first_gov_change_round);
        self.n_stayed_despotism += other.n_stayed_despotism;

        self.wars_declared_per_playergame.extend(other.wars_declared_per_playergame);
        self.aggressions_played_per_playergame.extend(other.aggressions_played_per_playergame);
        self.n_weakest_at_end_age2 += other.n_weakest_at_end_age2;
        self.n_playergames_with_age2_sample += other.n_playergames_with_age2_sample;

        for i in 0..5 {
            self.age_samples[i].append(&mut other.age_samples[i]);
        }
        self.final_score.extend(other.final_score);

        for i in 0..7 {
            self.civil_spend_early[i].merge(other.civil_spend_early[i]);
            self.civil_spend_late[i].merge(other.civil_spend_late[i]);
        }
        self.n_player_games_early += other.n_player_games_early;
        self.n_player_games_late += other.n_player_games_late;
        self.round6_early.append(&mut other.round6_early);
        self.round6_late.append(&mut other.round6_late);

        self.card_fate_early.merge(other.card_fate_early);
        self.card_fate_late.merge(other.card_fate_late);
        self.n_card_fate_mismatches += other.n_card_fate_mismatches;
        self.cards_taken_per_playergame_early.extend(other.cards_taken_per_playergame_early);
        self.cards_taken_per_playergame_late.extend(other.cards_taken_per_playergame_late);
        self.cards_still_in_hand_per_playergame_early.extend(other.cards_still_in_hand_per_playergame_early);
        self.cards_still_in_hand_per_playergame_late.extend(other.cards_still_in_hand_per_playergame_late);
        self.played_dwell_early.extend(other.played_dwell_early);
        self.played_dwell_late.extend(other.played_dwell_late);
        self.censored_dwell_early.extend(other.censored_dwell_early);
        self.censored_dwell_late.extend(other.censored_dwell_late);
        self.playable_turns_played_early.extend(other.playable_turns_played_early);
        self.playable_turns_played_late.extend(other.playable_turns_played_late);
        self.playable_turns_never_played_early.extend(other.playable_turns_never_played_early);
        self.playable_turns_never_played_late.extend(other.playable_turns_never_played_late);
        self.blocked_played_early.extend(other.blocked_played_early);
        self.blocked_played_late.extend(other.blocked_played_late);
        self.blocked_never_played_zero_early.extend(other.blocked_never_played_zero_early);
        self.blocked_never_played_zero_late.extend(other.blocked_never_played_zero_late);
    }
}

fn merge_map(a: &mut HashMap<&'static str, u64>, b: HashMap<&'static str, u64>) {
    for (k, v) in b {
        *a.entry(k).or_insert(0) += v;
    }
}

/// [`merge_pair_map`]'s twin for [`Report::production_by_round`]'s
/// (sum food, sum resources, n) accumulator -- same "add the fields
/// element-wise" merge, just one wider tuple.
fn merge_triple_map<K: Eq + std::hash::Hash>(a: &mut HashMap<K, (u64, u64, u64)>, b: HashMap<K, (u64, u64, u64)>) {
    for (k, (food, resources, n)) in b {
        let e = a.entry(k).or_insert((0, 0, 0));
        e.0 += food;
        e.1 += resources;
        e.2 += n;
    }
}

/// [`merge_triple_map`]'s twin for [`Report::alloc_by_round`]'s
/// [`AllocAccum`] -- same "add every field element-wise" merge.
fn merge_alloc_map<K: Eq + std::hash::Hash>(a: &mut HashMap<K, AllocAccum>, b: HashMap<K, AllocAccum>) {
    for (k, v) in b {
        let e = a.entry(k).or_default();
        e.farm_workers += v.farm_workers;
        e.mine_workers += v.mine_workers;
        e.urban_workers += v.urban_workers;
        e.mil_workers += v.mil_workers;
        e.free_workers += v.free_workers;
        e.staffed_workers += v.staffed_workers;
        e.best_farm_sum += v.best_farm_sum;
        e.best_mine_sum += v.best_mine_sum;
        e.n += v.n;
    }
}

/// [`merge_alloc_map`]'s twin for [`Report::tech_acq`]'s [`StageCounts`] --
/// same "add every field element-wise" merge.
fn merge_techacq_map(a: &mut HashMap<TechKind, StageCounts>, b: HashMap<TechKind, StageCounts>) {
    for (k, v) in b {
        let e = a.entry(k).or_default();
        e.seen += v.seen;
        e.taken += v.taken;
        e.built += v.built;
        e.staffed += v.staffed;
    }
}

/// Element-wise merge for [`Report::farm_tier`]/[`Report::mine_tier`].
fn merge_tier_counts(a: &mut TierCounts, b: TierCounts) {
    for i in 0..4 {
        a.taken[i] += b.taken[i];
        a.built[i] += b.built[i];
    }
}

/// Generic (wins, total) accumulator merge -- shared by every
/// `opening_*` map in [`Report`], whose keys differ (`&'static str`,
/// `bool`) but whose value shape (a running win/total pair, so a win-rate
/// can be printed with its own sample size) does not.
fn merge_pair_map<K: Eq + std::hash::Hash>(a: &mut HashMap<K, (u64, u64)>, b: HashMap<K, (u64, u64)>) {
    for (k, (w, t)) in b {
        let e = a.entry(k).or_insert((0, 0));
        e.0 += w;
        e.1 += t;
    }
}

/// Generic plain-count map merge (no win/total pairing) -- used by
/// [`Report::opening_ca_unused`], keyed by `i32`.
fn merge_count_map<K: Eq + std::hash::Hash>(a: &mut HashMap<K, u64>, b: HashMap<K, u64>) {
    for (k, v) in b {
        *a.entry(k).or_insert(0) += v;
    }
}

/// Per-player, per-game opening/wonder/government tracking state, carried
/// across the move loop for one game.
struct PlayerTrack {
    first_take: Option<CardId>,
    first_develop: Option<(CardId, u16)>,
    first_wonder_round: Option<u16>,
    wonder_started: HashMap<CardId, ()>,
    /// Fate of every distinct wonder that has already LEFT this player's
    /// `wonder` slot (completed or destroyed). A `wonder_started` entry with
    /// no matching key here is still in the slot -- resolved to
    /// [`WonderFate::StillInProgress`] at end of game.
    wonder_fate: HashMap<CardId, WonderFate>,
    gov_changes: u32,
    first_gov_change_round: Option<u16>,
    wars_declared: u32,
    aggressions: u32,

    // ---- opening, human-comparable schema (`bin/humanopenings.rs`'s own
    // 10-column TSV: game_id/seat/first_take_name/first_take_cost/
    // first_build_kind/first_build_name/leader_by_r3/pop_by_r3/
    // ca_unused_by_r3/outcome) -- kept as SEPARATE fields from the ones
    // above (rather than reusing `first_take`/`first_wonder_round`) so this
    // binary's pre-existing opening report is untouched and the new
    // divergence-table comparison reads off an identical definition on both
    // sides, not two definitions that happen to look similar.
    /// Separate from `first_take` above on purpose: `first_take` is gated
    /// on `pre_age == Age::A` (this file's own pre-existing convention),
    /// while this one matches `bin/humanopenings.rs`'s gate exactly
    /// (`round_before <= 3`, no age restriction) -- Age A can end before
    /// round 3 in a fast game, so the two conditions are NOT the same set
    /// of moves and must not share a field.
    first_take_name: Option<&'static str>,
    first_take_cost: Option<i32>,
    first_build: Option<(&'static str, &'static str)>,
    took_leader_r3: bool,
    increased_pop_r3: bool,
    ca_unused_r3: i32,

    /// Civil-action spend by [`CivilMoveKind`], rounds 3-9 (index 0..7 =
    /// round 3..9) -- accumulated regardless of this player-game's eventual
    /// [`WonderTempoGroup`], which is not known until the game ends; folded
    /// into `Report::civil_spend_early`/`civil_spend_late` at that point.
    civil_spend_by_round: [CivilSpendCounts; 7],
    /// This player's board, sampled PRE-move at the move that ends round 6
    /// (same "state before the transition" convention as the age-boundary
    /// snapshots above) -- `None` if the game ended before round 6 was
    /// reached.
    round6_sample: Option<AgeSample>,

    /// Card-fate tracking (see the "Card fate" section above): a FIFO
    /// queue of "round taken" per distinct `CardId` this player has ever
    /// held in `hand_civil`, so a card's eventual fate can be matched back
    /// to the specific take that put it in hand. An entry is pushed by a
    /// `Move::Take` and popped by exactly one of the "played" or
    /// "antiquated" event blocks in `play_one`'s move loop; whatever is
    /// left in the queues at game end is `StillInHand`.
    taken_rounds: HashMap<CardId, VecDeque<TakenCard>>,
    n_taken: u32,
    n_played: u32,
    n_antiquated: u32,
    /// See `n_card_fate_mismatches` on [`Report`].
    n_card_fate_mismatch: u32,
    played_dwell: Vec<i32>,
    /// Antiquation-censored dwell only (rounds held before being culled) --
    /// the still-in-hand-at-game-end half of the censored population is
    /// computed once, at game end, from whatever is left in `taken_rounds`
    /// (see `play_one`'s end-of-game card-fate block), not accumulated here.
    censored_dwell: Vec<i32>,
    /// `TakenCard::playable_turns`, one sample per PLAYED card, pushed at
    /// the same call site as `played_dwell` -- the control population: how
    /// many decision points a card sat legal before it was actually played.
    playable_turns_played: Vec<u32>,
    /// The same, for cards resolved ANTIQUATED -- blended with the
    /// still-in-hand-at-game-end population into `Report::playable_turns_
    /// never_played_*` at game end, matching how `censored_dwell` already
    /// blends antiquated + still-in-hand into one "never played" population.
    playable_turns_antiquated: Vec<u32>,
    /// One `(blocked_no_civil_action, blocked_nothing_affordable,
    /// blocked_something_else_
    /// developable)` pair per PLAYED card, pushed at the same call site as
    /// `playable_turns_played` -- the control population for the poverty/
    /// selection-loss follow-up below.
    blocked_played: Vec<(u32, u32, u32)>,
    /// The same pair, for cards resolved ANTIQUATED with zero playable-
    /// turns -- blended with the still-in-hand-at-game-end half (also
    /// filtered to zero playable-turns) into `Report::blocked_never_
    /// played_zero_*` at game end, matching how `playable_turns_antiquated`
    /// already blends into `playable_turns_never_played_*`.
    blocked_zero_antiquated: Vec<(u32, u32, u32)>,

    /// Distinct cards ever SEEN in the card row at this player's own
    /// turn-start samples, TAKEN via `Move::Take`, BUILT into `techs`, and
    /// STAFFED (workers > 0) at a turn-start sample -- see the "Tech
    /// acquisition" section's doc comment (above `TechKind`) for exactly
    /// what each stage measures and which of this file's existing hooks it
    /// is read from. Resolved into `Report::tech_acq`/`farm_tier`/
    /// `mine_tier` once at game end (`play_one`'s end-of-game loop),
    /// matching how `wonder_started` above is also a running set resolved
    /// once at the end.
    tech_seen: HashSet<CardId>,
    tech_taken: HashSet<CardId>,
    tech_built: HashSet<CardId>,
    tech_staffed: HashSet<CardId>,
}

impl PlayerTrack {
    fn new() -> Self {
        PlayerTrack {
            first_take: None,
            first_develop: None,
            first_wonder_round: None,
            wonder_started: HashMap::new(),
            wonder_fate: HashMap::new(),
            gov_changes: 0,
            first_gov_change_round: None,
            wars_declared: 0,
            aggressions: 0,
            first_take_name: None,
            first_take_cost: None,
            first_build: None,
            took_leader_r3: false,
            increased_pop_r3: false,
            ca_unused_r3: 0,
            civil_spend_by_round: [CivilSpendCounts::default(); 7],
            round6_sample: None,
            taken_rounds: HashMap::new(),
            n_taken: 0,
            n_played: 0,
            n_antiquated: 0,
            n_card_fate_mismatch: 0,
            played_dwell: Vec::new(),
            censored_dwell: Vec::new(),
            blocked_played: Vec::new(),
            blocked_zero_antiquated: Vec::new(),
            playable_turns_played: Vec::new(),
            playable_turns_antiquated: Vec::new(),
            tech_seen: HashSet::new(),
            tech_taken: HashSet::new(),
            tech_built: HashSet::new(),
            tech_staffed: HashSet::new(),
        }
    }
}

/// Maps a built/developed/upgraded-into card's [`CardType`] to the same
/// coarse bucket names `bin/humanopenings.rs::build_kind` uses, so the two
/// binaries' "first build kind" distributions are directly comparable.
fn opening_build_kind(kind: tta::CardType) -> &'static str {
    use tta::CardType;
    match kind {
        CardType::Farm => "Farm",
        CardType::Mine => "Mine",
        CardType::Temple => "Temple",
        CardType::Lab => "Lab",
        CardType::Library => "Library",
        CardType::Arena => "Arena",
        CardType::Theater => "Theater",
        CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => "Military",
        CardType::Wonder => "WonderStage",
        CardType::Government | CardType::SpecialTech | CardType::Leader | CardType::Action | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => "Other",
    }
}

/// The victim whose wonder `mv` MIGHT be about to clear via Infiltrate, if
/// any -- structural, not a `Move::Choose` value sniff, because
/// `combat::finish_aggression`'s single `Special::RemoveFromGame` gate
/// (which enqueues `QueueItem::Infiltrate`) can resolve on three different
/// move shapes depending on how much decision-making it needs along the
/// way:
///
///   1. `Move::Aggression` itself, when the defender has no military
///      actions or an empty hand to spend: `interact::start_defense`'s
///      short-circuit calls `finish_aggression` inline, in the very step
///      that declared the aggression, with no `Pending::Defense` ever
///      created to read.
///   2. `Move::Defend` / `Move::DefendDone`, when the defender DOES have
///      cards to spend: `state.pending` carries `Pending::Defense { card,
///      player, .. }` (`interact::start_defense` / `defense_move`) right up
///      until the move that exhausts the defender's budget or hand, or that
///      passes voluntarily, pops it and calls `finish_aggression` there.
///   3. `Move::Choose`, only when the victim has BOTH a leader and a
///      wonder: `QueueItem::Infiltrate`'s handler (interact.rs) then offers
///      a genuine two-option decision, answered by a later move.
///      `state.pending` carries `Pending::Choice(Choice { kind:
///      ChoiceKind::Infiltrate { victim, .. }, .. })` up until that move.
///      (A leaderless victim's one-option list auto-resolves inside
///      whichever of cases 1/2 enqueued it -- `push_choice(..., auto:
///      true)`'s `options.len() == 1` branch -- so no `Move::Choose` is
///      emitted for that shape at all; this arm exists for the two-option
///      shape only.)
///
/// Naming a victim here for a case that turns out NOT to resolve on this
/// exact move (a `Pending::Defense` gets pushed instead of resolving inline
/// in case 1; the attacker's answer lands on `Leader` rather than `Wonder`
/// in case 3) is harmless: the caller only consults this when the victim's
/// `.wonder` is already known to have changed on THIS move, and none of
/// those non-resolving cases change it.
fn infiltrate_candidate_victim(state: &tta::GameState, mv: Move) -> Option<u8> {
    if let Move::Aggression { card, target } = mv {
        if card.get().special.contains(&Special::RemoveFromGame) {
            return Some(target);
        }
    }
    match state.pending.top() {
        Some(Pending::Defense(d)) if d.card.get().special.contains(&Special::RemoveFromGame) => Some(d.player),
        Some(Pending::Choice(c)) => match c.kind {
            ChoiceKind::Infiltrate { victim, .. } => Some(victim),
            ChoiceKind::GainBlock | ChoiceKind::FreeCivil { .. } | ChoiceKind::FoodOrRes { .. } | ChoiceKind::FreeBuild | ChoiceKind::DestroyOwn | ChoiceKind::LosePop | ChoiceKind::LoseColony | ChoiceKind::FlipWonder | ChoiceKind::DiscardMilitary | ChoiceKind::Raid { .. } | ChoiceKind::Annex { .. } | ChoiceKind::PactOffer { .. } | ChoiceKind::TakeRow { .. } | ChoiceKind::WarTech { .. } | ChoiceKind::PlunderSplit { .. } | ChoiceKind::FoodOrResSplit { .. } => None,
        },
        _ => None,
    }
}

/// The fate of a single player's wonder that is KNOWN to have left the
/// `wonder` slot on this move (caller already checked `before != after`).
/// Pulled out of `play_one`'s move loop so the classification itself --
/// order of precedence completed > infiltrated > antiquated > unexplained --
/// is unit-testable without driving a real game.
fn classify_wonder_change(
    completed_this_move: bool,
    pending_infiltrate_victim: Option<u8>,
    victim: u8,
    age_changed_this_move: bool,
) -> WonderFate {
    if completed_this_move {
        WonderFate::Completed
    } else if pending_infiltrate_victim == Some(victim) {
        WonderFate::DestroyedByInfiltrate
    } else if age_changed_this_move {
        WonderFate::DestroyedByAntiquation
    } else {
        WonderFate::DestroyedUnexplained
    }
}

fn play_one(players: u8, weights: Weights, seed: u64) -> (Report, bool) {
    let seats: Vec<Seat> =
        (0..players).map(|_| Seat { kind: BotKind::Weighted, weights, search: Search::None }).collect();
    let mut bots = build_bots(&seats, seed as i64);
    let mut state = game::new_game(players, seed);

    let mut report = Report::default();
    let mut tracks: Vec<PlayerTrack> = (0..players).map(|_| PlayerTrack::new()).collect();
    let mut moves_played = 0usize;
    let mut cap_hit = false;
    let mut prev_age = state.age_civil;
    // Round-6 wonder-tempo board snapshot boundary -- mirrors `prev_age`
    // above exactly, just keyed on `state.round` instead of `state.age_civil`.
    let mut prev_round = state.round;
    // Production-curve boundary: the actor whose start-of-turn sample was
    // last taken. `state.current` is reassigned exactly once per turn, at
    // `end_turn`'s `state.current = nxt` (game.rs) -- nothing in a combat/
    // pending exchange ever reassigns it -- so "the actor differs from the
    // last one sampled" is exactly "this is the first move of a new turn",
    // with no confound from Defend/Bid/Choose moves along the way.
    let mut prev_actor: Option<u8> = None;

    while !state.game_over {
        if moves_played >= MOVE_CAP {
            cap_hit = true;
            break;
        }
        let mv = bots[state.current as usize].pick(&state);
        let actor = state.current;
        let round_before = state.round;

        // ---- production curve: sample the FIRST move of every new turn,
        // before `mv` is applied -- see `prev_actor`'s doc comment above for
        // why "actor changed" is exactly "turn boundary" here.
        if prev_actor != Some(actor) {
            prev_actor = Some(actor);
            let (food, resources) = tta::economy::production_this_turn(&state, actor);
            let e = report.production_by_round.entry(round_before).or_insert((0, 0, 0));
            e.0 += u64::from(food);
            e.1 += u64::from(resources);
            e.2 += 1;

            // ---- worker allocation: SAME instant, same player, additionally
            // classifying every placed worker by `CardType` and reading the
            // printed per-worker production of every Farm/Mine tech held
            // (whether staffed or not -- a level "held" is a tech OWNED, not
            // a worker placed on it). Exhaustive match, no wildcard arm
            // (`wildcard_enum_match_arm` is denied repo-wide).
            let p = &state.players[actor as usize];
            let mut farm_workers = 0u32;
            let mut mine_workers = 0u32;
            let mut urban_workers = 0u32;
            let mut mil_workers = 0u32;
            let mut best_farm = 0i16;
            let mut best_mine = 0i16;
            for (id, slot) in p.techs.iter() {
                let workers = u32::from(slot.workers);
                // ---- tech acquisition: BUILT/STAFFED -- same turn-start
                // instant, same `p.techs` walk as the alloc classification
                // just below; see the "Tech acquisition" section's doc
                // comment (above `TechKind`) for why these two stages are
                // read here rather than at a new sample point.
                if tech_kind(id.get().kind).is_some() {
                    tracks[actor as usize].tech_built.insert(id);
                    if workers > 0 {
                        tracks[actor as usize].tech_staffed.insert(id);
                    }
                }
                match id.get().kind {
                    CardType::Farm => {
                        farm_workers += workers;
                        best_farm = best_farm.max(id.get().production.food);
                    }
                    CardType::Mine => {
                        mine_workers += workers;
                        best_mine = best_mine.max(id.get().production.resources);
                    }
                    CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater => {
                        urban_workers += workers;
                    }
                    CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => {
                        mil_workers += workers;
                    }
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
                    | CardType::Event => {}
                }
            }
            let a = report.alloc_by_round.entry(round_before).or_default();
            a.farm_workers += u64::from(farm_workers);
            a.mine_workers += u64::from(mine_workers);
            a.urban_workers += u64::from(urban_workers);
            a.mil_workers += u64::from(mil_workers);
            a.free_workers += u64::from(p.workers_free);
            a.staffed_workers += u64::from(farm_workers + mine_workers + urban_workers + mil_workers);
            a.best_farm_sum += u64::from(best_farm.max(0) as u16);
            a.best_mine_sum += u64::from(best_mine.max(0) as u16);
            a.n += 1;

            // ---- tech acquisition: SEEN -- same turn-start instant, every
            // distinct tracked-type card visible in the card row right now
            // (see the "Tech acquisition" section's doc comment above
            // `TechKind`).
            for &card in &state.card_row {
                if !card.is_none() && tech_kind(card.get().kind).is_some() {
                    tracks[actor as usize].tech_seen.insert(card);
                }
            }
        }

        // pre-move info needed for classification
        let taken_card = match mv {
            Move::Take { slot } => Some(state.card_row[slot as usize]),
            Move::Build { .. } | Move::Develop { .. } | Move::Upgrade { .. } | Move::WonderStep { .. } | Move::Pop { .. } | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => None,
        };
        // Cost must be read PRE-move (`tta::costs::take_cost` reads the
        // row/player state a `Move::Take` is about to consume) -- same
        // reason `bin/humanopenings.rs` reads it off `d.state` before
        // applying the human's move, not after.
        let taken_card_cost = match mv {
            Move::Take { slot } => Some(tta::costs::take_cost(&state, &state.players[actor as usize], slot as usize)),
            Move::Build { .. } | Move::Develop { .. } | Move::Upgrade { .. } | Move::WonderStep { .. } | Move::Pop { .. } | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => None,
        };
        let ca_before_move = state.players[actor as usize].civil_actions;
        let pre_govt = state.players[actor as usize].government;
        let pre_age = state.age_civil;
        // Card-fate: the CardId this move plays out of hand_civil, if any
        // (see played_civil_card's doc comment) -- computed pre-move
        // because it only reads `mv` itself, and is needed both after
        // game::step (to resolve the "played" fate) and inside the
        // antiquation hand-diff below (to avoid double-counting the same
        // card as both played and antiquated on the one move that could
        // ever coincide with an age transition).
        let played_this_move = played_civil_card(mv);

        // pre-move snapshot for every player, used only if this move is the
        // one that advances state.age_civil.
        let pre_snapshots: Vec<AgeSample> = (0..players).map(|i| sample_player(&state, i)).collect();
        // pre-move hand_civil snapshot for every player, used only if this
        // move is the one that advances state.age_civil -- same "compute
        // for every player, use conditionally" idiom as pre_snapshots just
        // above, so antiquation's hand-diff (see hand_multiset_diff) can
        // read what left EVERY player's hand this move, not just the actor's.
        let pre_hands: Vec<Vec<CardId>> =
            (0..players).map(|i| state.players[i as usize].hand_civil.as_slice().to_vec()).collect();

        // pre-move wonder-fate inputs: who holds what wonder right now, how
        // many each has completed so far, and whether `mv` might be about
        // to clear some victim's wonder via Infiltrate -- see
        // `infiltrate_candidate_victim`'s doc comment for why this has to
        // be read off THREE different move shapes rather than just
        // `Move::Choose`, and why over-naming a victim here is harmless.
        let before_wonder: Vec<CardId> = (0..players).map(|i| state.players[i as usize].wonder).collect();
        let before_completed_len: Vec<usize> =
            (0..players).map(|i| state.players[i as usize].completed_wonders.len()).collect();
        let pending_infiltrate_victim: Option<u8> = infiltrate_candidate_victim(&state, mv);

        // ---- card fate: playable-turns -- for every distinct civil card
        // already sitting in the ACTING player's hand at this exact
        // decision point (before `mv` is applied), was Develop/PlayLeader/
        // Revolution/PlayAction of it in `legal::legal_moves` right now?
        // This is the same call `Seat::pick` above already made internally
        // to choose `mv` -- see legal.rs:214's "single source of truth" doc
        // comment -- so it can never drift from what the bot was actually
        // allowed to do here, unlike re-deriving affordability from
        // `costs.rs`. One increment per distinct `CardId` per decision
        // point, not per physical copy: `legal_moves` itself only ever
        // offers one Develop/PlayLeader/Revolution/PlayAction move per
        // distinct card (`legal.rs`'s hand iteration dedupes via
        // `sorted_unique_into`), so every outstanding copy of that card in
        // `taken_rounds` is equally playable right now.
        {
            let legal_now = legal::legal_moves(&state);
            let mut playable_cards: Vec<CardId> = Vec::new();
            for &m in legal_now.as_slice() {
                if let Some(card) = played_civil_card(m) {
                    if !playable_cards.contains(&card) {
                        playable_cards.push(card);
                    }
                }
            }
            // Was ANY civil card in this player's hand developable right
            // now? Read straight off the `playable_cards` scan just above
            // -- the same `legal_now` MoveList, no second `legal::
            // legal_moves` call -- so a card that was NOT legal can be
            // split into "nothing in hand was legal either" (poverty) vs.
            // "something else in hand WAS legal" (this card specifically
            // lost the auction).
            let any_civil_playable = !playable_cards.is_empty();
            // ... and when nothing was legal, WHY: no action left to spend,
            // or an action in hand that nothing could be paid for. Both look
            // identical in `legal_moves` (a Develop the player cannot pay
            // the CA for is simply absent) and they have opposite fixes, so
            // the budget has to be read off the player directly.
            let has_civil_action = state.players[actor as usize].civil_actions > 0;
            let hand_now = state.players[actor as usize].hand_civil.as_slice();
            let mut credited: Vec<CardId> = Vec::new();
            for &card in hand_now {
                if credited.contains(&card) {
                    continue;
                }
                credited.push(card);
                if let Some(queue) = tracks[actor as usize].taken_rounds.get_mut(&card) {
                    for entry in queue.iter_mut() {
                        if playable_cards.contains(&card) {
                            entry.playable_turns += 1;
                        } else if any_civil_playable {
                            entry.blocked_something_else_developable += 1;
                        } else if has_civil_action {
                            entry.blocked_nothing_affordable += 1;
                        } else {
                            entry.blocked_no_civil_action += 1;
                        }
                    }
                }
            }
        }

        game::step(&mut state, mv);
        moves_played += 1;

        // ---- wonder fate: resolve whatever LEFT the slot this move ----
        // Antiquation (`game::antiquate`) runs only on the move that
        // advances `state.age_civil` (`game.rs`'s age-transition branch
        // calls it exactly there), so an age change on this exact move is
        // how a census loop -- which has no other hook into that private
        // function -- can attribute a same-move clearing to it.
        let age_changed_this_move = state.age_civil != pre_age;
        for i in 0..players {
            let before = before_wonder[i as usize];
            let after = state.players[i as usize].wonder;
            if before.is_none() || before == after {
                continue;
            }
            let completed_this_move =
                state.players[i as usize].completed_wonders.len() > before_completed_len[i as usize];
            let fate = classify_wonder_change(completed_this_move, pending_infiltrate_victim, i, age_changed_this_move);
            tracks[i as usize].wonder_fate.entry(before).or_insert(fate);
        }

        // ---- card fate: TAKEN -- every non-wonder card a Move::Take put
        // into hand_civil this move (a wonder goes straight to the
        // `.wonder` slot, never hand_civil -- see apply::take_card_impl,
        // same fact the wonder-fate section above depends on).
        if let Some(card) = taken_card {
            if !card.is_none() && card.get().kind != CardType::Wonder {
                tracks[actor as usize]
                    .taken_rounds
                    .entry(card)
                    .or_default()
                    .push_back(TakenCard {
                        taken_round: round_before,
                        playable_turns: 0,
                        blocked_no_civil_action: 0,
                        blocked_nothing_affordable: 0,
                        blocked_something_else_developable: 0,
                    });
                tracks[actor as usize].n_taken += 1;
            }
        }

        // ---- card fate: PLAYED -- the card played_this_move named, if
        // any, matched back to the take that put it in hand (see
        // played_civil_card's doc comment for which four Move variants
        // this can ever be non-None for).
        if let Some(card) = played_this_move {
            let t = &mut tracks[actor as usize];
            match t.taken_rounds.get_mut(&card).and_then(VecDeque::pop_front) {
                Some(TakenCard { taken_round, playable_turns, blocked_no_civil_action, blocked_nothing_affordable, blocked_something_else_developable }) => {
                    t.n_played += 1;
                    t.played_dwell.push(round_before as i32 - taken_round as i32);
                    t.playable_turns_played.push(playable_turns);
                    t.blocked_played.push((blocked_no_civil_action, blocked_nothing_affordable, blocked_something_else_developable));
                }
                None => t.n_card_fate_mismatch += 1,
            }
        }

        // ---- card fate: ANTIQUATED -- civil cards culled from a hand by
        // the same age-transition this move already detected for
        // WonderFate::DestroyedByAntiquation above. Read as a pre/post
        // hand_civil multiset diff (see hand_multiset_diff) rather than
        // reimplementing game::antiquate_hands's own age-cutoff test,
        // because that keeps this census reading what the engine actually
        // did rather than a second, potentially-drifting copy of the rule.
        // The actor's OWN played card this move (if any) is excluded from
        // the diff first: it already left hand_civil via the PLAYED branch
        // above, on the very same move, and must not be double-counted as
        // antiquated too.
        if age_changed_this_move {
            for i in 0..players {
                let post_hand = state.players[i as usize].hand_civil.as_slice();
                let mut removed = hand_multiset_diff(&pre_hands[i as usize], post_hand);
                if i == actor {
                    if let Some(played) = played_this_move {
                        if let Some(pos) = removed.iter().position(|&c| c == played) {
                            removed.remove(pos);
                        }
                    }
                }
                let t = &mut tracks[i as usize];
                for card in removed {
                    match t.taken_rounds.get_mut(&card).and_then(VecDeque::pop_front) {
                        Some(TakenCard { taken_round, playable_turns, blocked_no_civil_action, blocked_nothing_affordable, blocked_something_else_developable }) => {
                            t.n_antiquated += 1;
                            t.censored_dwell.push(round_before as i32 - taken_round as i32);
                            t.playable_turns_antiquated.push(playable_turns);
                            // Only the zero-playable-turns population feeds
                            // the poverty/selection-loss follow-up below --
                            // a card that WAS legal at least once already
                            // has its story told by playable_turns itself.
                            if playable_turns == 0 {
                                t.blocked_zero_antiquated
                                    .push((blocked_no_civil_action, blocked_nothing_affordable, blocked_something_else_developable));
                            }
                        }
                        None => t.n_card_fate_mismatch += 1,
                    }
                }
            }
        }

        // ---- wonder tempo: civil-action spend by move kind, rounds 3-9
        // (see the "Wonder-tempo EARLY/LATE grouping" section above). Spend
        // is read as the ACTUAL post-move drop in `civil_actions`, not a
        // flat 1 per move, so a discounted/free move (a banked civil-life
        // grant, an action-card discount) correctly contributes 0 -- the
        // same reasoning `ca_before_move` above already relies on for the
        // round<=3 `ca_unused_r3` bucket, just read on the other side of
        // `game::step` too.
        if (3..=9).contains(&round_before) {
            let ca_after_move = state.players[actor as usize].civil_actions;
            let ca_spent = (ca_before_move as i32 - ca_after_move as i32).max(0) as u64;
            if ca_spent > 0 {
                let idx = (round_before - 3) as usize;
                tracks[actor as usize].civil_spend_by_round[idx].record(civil_move_kind(mv), ca_spent);
            }
        }

        // ---- openings ----
        if let Some(card) = taken_card {
            if !card.is_none() && pre_age == Age::A && tracks[actor as usize].first_take.is_none() {
                tracks[actor as usize].first_take = Some(card);
            }
        }
        // ---- tech acquisition: TAKEN -- reuses the same `taken_card` this
        // file already computes pre-move for the opening block just above,
        // for every decision point (not just turn-start: a Take can happen
        // anywhere in a turn). See the "Tech acquisition" section's doc
        // comment above `TechKind`.
        if let Some(card) = taken_card {
            if !card.is_none() && tech_kind(card.get().kind).is_some() {
                tracks[actor as usize].tech_taken.insert(card);
            }
        }
        if let Move::Develop { card, .. } = mv {
            if tracks[actor as usize].first_develop.is_none() {
                tracks[actor as usize].first_develop = Some((card, round_before));
            }
        }
        if let Move::WonderStep { .. } = mv {
            if tracks[actor as usize].first_wonder_round.is_none() {
                tracks[actor as usize].first_wonder_round = Some(round_before);
            }
        }
        if let Move::Revolution { .. } = mv {
            let t = &mut tracks[actor as usize];
            t.gov_changes += 1;
            if t.first_gov_change_round.is_none() {
                t.first_gov_change_round = Some(round_before);
            }
        }
        let _ = pre_govt; // government identity change is implied by Revolution above
        if let Move::War { .. } = mv {
            tracks[actor as usize].wars_declared += 1;
        }
        if let Move::Aggression { .. } = mv {
            tracks[actor as usize].aggressions += 1;
        }

        // ---- opening, human-comparable schema (round_before <= 3 only,
        // matching `bin/humanopenings.rs`'s own `d.state.round > 3`
        // filter -- both read the round the decision was MADE in, not the
        // round it resolved in) ----
        if round_before <= 3 {
            let t = &mut tracks[actor as usize];
            if let (Move::Take { .. }, Some(card), Some(cost)) = (mv, taken_card, taken_card_cost) {
                if !card.is_none() && t.first_take_name.is_none() {
                    t.first_take_name = Some(card.get().name);
                    t.first_take_cost = Some(cost);
                }
            }
            match mv {
                Move::Build { card } | Move::Develop { card, .. } => {
                    if t.first_build.is_none() {
                        t.first_build = Some((opening_build_kind(card.get().kind), card.get().name));
                    }
                }
                Move::Upgrade { to, .. } => {
                    if t.first_build.is_none() {
                        t.first_build = Some((opening_build_kind(to.get().kind), to.get().name));
                    }
                }
                Move::PlayLeader { .. } => t.took_leader_r3 = true,
                Move::Pop { .. } | Move::PopFree => t.increased_pop_r3 = true,
                Move::EndTurn => {
                    if ca_before_move > 0 {
                        t.ca_unused_r3 += ca_before_move as i32;
                    }
                }
                Move::Take { .. } | Move::WonderStep { .. } | Move::Revolution { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::PolPass | Move::Resign => {}
            }
        }

        // ---- wonder-in-progress tracking (post-move state) ----
        for i in 0..players {
            let w = state.players[i as usize].wonder;
            if !w.is_none() {
                tracks[i as usize].wonder_started.entry(w).or_insert(());
            }
        }

        // ---- age-boundary snapshot ----
        if state.age_civil != prev_age {
            let idx = age_index(prev_age);
            report.age_samples[idx].extend(pre_snapshots.iter().copied());
            prev_age = state.age_civil;
        }

        // ---- round-6 boundary snapshot (wonder-tempo EARLY/LATE board
        // comparison) -- same PRE-move convention as the age-boundary
        // snapshot just above, keyed on `state.round` instead. Stashed on
        // `tracks` rather than `report` directly because the EARLY/LATE
        // group isn't known until the game ends (`wonder_tempo_group` reads
        // `first_wonder_round`, which is still being written this move).
        if state.round != prev_round {
            if prev_round == 6 {
                for (i, sample) in pre_snapshots.iter().enumerate() {
                    tracks[i].round6_sample = Some(*sample);
                }
            }
            prev_round = state.round;
        }
    }

    // ---- final / end-of-game bookkeeping ----
    let final_idx = age_index(Age::IV);
    let final_snapshots: Vec<AgeSample> = (0..players).map(|i| sample_player(&state, i)).collect();
    report.age_samples[final_idx].extend(final_snapshots);
    // Card fate: the round every still-in-hand taken card's censored dwell
    // (rounds held so far, never played) is measured against.
    let final_round = state.round;

    // Win/loss for the opening win-rate maps below: the STRICT max of
    // `PlayerState::culture` (this file's own final-score field, see
    // `report.final_score.push` below) among this game's seats, unknown
    // (excluded) on a `cap_hit` game (no `state.game_over`, so there is no
    // real winner) or an exact tie -- matching `bin/humanopenings.rs`'s own
    // "unknown" bucket for a game its outcome parse could not resolve.
    let final_culture: Vec<i32> = (0..players).map(|i| state.players[i as usize].culture as i32).collect();
    let max_culture = final_culture.iter().copied().max();
    let is_winner = |i: usize| -> Option<bool> {
        if cap_hit {
            return None;
        }
        let max_c = max_culture?;
        if final_culture.iter().filter(|&&c| c == max_c).count() != 1 {
            return None; // tie -- no single winner, matching humanopenings' "tie" exclusion
        }
        Some(final_culture[i] == max_c)
    };

    for i in 0..players {
        let p = &state.players[i as usize];
        report.n_player_games += 1;

        // ---- opening, human-comparable schema: record into (wins, total)
        // maps, or the unknown-outcome counter, per player-game.
        {
            let t = &tracks[i as usize];
            let win = is_winner(i as usize);
            match win {
                Some(w) => {
                    let take_key = t.first_take_name.unwrap_or("none");
                    let e = report.opening_first_take.entry(take_key).or_insert((0, 0));
                    e.1 += 1;
                    if w {
                        e.0 += 1;
                    }
                    let build_key = t.first_build.map(|(k, _)| k).unwrap_or("none");
                    let e = report.opening_first_build_kind.entry(build_key).or_insert((0, 0));
                    e.1 += 1;
                    if w {
                        e.0 += 1;
                    }
                    let e = report.opening_leader_r3.entry(t.took_leader_r3).or_insert((0, 0));
                    e.1 += 1;
                    if w {
                        e.0 += 1;
                    }
                    let e = report.opening_pop_r3.entry(t.increased_pop_r3).or_insert((0, 0));
                    e.1 += 1;
                    if w {
                        e.0 += 1;
                    }
                }
                None => report.opening_outcome_unknown += 1,
            }
            *report.opening_ca_unused.entry(t.ca_unused_r3).or_insert(0) += 1;
            // Outside the win/unknown match on purpose: a price is a fact about
            // the move, not about how the game ended, so it is sampled from
            // every player-game exactly as `ca_unused_r3` is.
            if let Some(cost) = t.first_take_cost {
                *report.opening_first_take_cost.entry(cost).or_insert(0) += 1;
            }
        }

        if let Some(card) = tracks[i as usize].first_take {
            *report.first_take_card.entry(card.name()).or_insert(0) += 1;
        }
        if let Some((card, round)) = tracks[i as usize].first_develop {
            *report.first_develop_card.entry(card.name()).or_insert(0) += 1;
            report.first_develop_round.push(round as i32);
        }

        // ---- tech acquisition: resolve this player-game's four running
        // sets into `Report::tech_acq`/`farm_tier`/`mine_tier` -- see the
        // "Tech acquisition" section's doc comment above `TechKind`.
        {
            let t = &tracks[i as usize];
            for &card in &t.tech_seen {
                if let Some(kind) = tech_kind(card.get().kind) {
                    report.tech_acq.entry(kind).or_default().seen += 1;
                }
            }
            for &card in &t.tech_taken {
                if let Some(kind) = tech_kind(card.get().kind) {
                    report.tech_acq.entry(kind).or_default().taken += 1;
                    let tier = card.level() as usize;
                    match kind {
                        TechKind::Farm => {
                            if let Some(slot) = report.farm_tier.taken.get_mut(tier) {
                                *slot += 1;
                            }
                        }
                        TechKind::Mine => {
                            if let Some(slot) = report.mine_tier.taken.get_mut(tier) {
                                *slot += 1;
                            }
                        }
                        TechKind::Lab | TechKind::Temple | TechKind::Arena | TechKind::Library | TechKind::Military => {}
                    }
                }
            }
            for &card in &t.tech_built {
                if let Some(kind) = tech_kind(card.get().kind) {
                    report.tech_acq.entry(kind).or_default().built += 1;
                    let tier = card.level() as usize;
                    match kind {
                        TechKind::Farm => {
                            if let Some(slot) = report.farm_tier.built.get_mut(tier) {
                                *slot += 1;
                            }
                        }
                        TechKind::Mine => {
                            if let Some(slot) = report.mine_tier.built.get_mut(tier) {
                                *slot += 1;
                            }
                        }
                        TechKind::Lab | TechKind::Temple | TechKind::Arena | TechKind::Library | TechKind::Military => {}
                    }
                }
            }
            for &card in &t.tech_staffed {
                if let Some(kind) = tech_kind(card.get().kind) {
                    report.tech_acq.entry(kind).or_default().staffed += 1;
                }
            }
        }
        // Win-rate bucket, same (wins, total) shape and same "unresolvable
        // outcome is excluded" rule as the `opening_first_take` /
        // `opening_first_build_kind` maps above -- `n_player_games_no_wonder_
        // by_round4` stays unconditional (every player-game, resolvable or
        // not), matching its pre-existing definition.
        let win = is_winner(i as usize);
        match tracks[i as usize].first_wonder_round {
            Some(r) => {
                if r > 4 {
                    report.n_player_games_no_wonder_by_round4 += 1;
                }
                if let Some(w) = win {
                    let e = report.first_wonder_round.entry(r as i32).or_insert((0, 0));
                    e.1 += 1;
                    if w {
                        e.0 += 1;
                    }
                }
            }
            None => {
                report.n_player_games_no_wonder_by_round4 += 1;
                if let Some(w) = win {
                    report.first_wonder_never_built.1 += 1;
                    if w {
                        report.first_wonder_never_built.0 += 1;
                    }
                }
            }
        }

        // ---- wonder-tempo EARLY/LATE cross-tab: fold this player-game's
        // rounds-3-9 civil-action spend and round-6 board sample into
        // whichever group its first-wonder-stage round belongs to (see
        // `wonder_tempo_group`'s doc comment for the boundary and the
        // excluded round-6-9 middle band).
        if let Some(group) = wonder_tempo_group(tracks[i as usize].first_wonder_round) {
            match group {
                WonderTempoGroup::Early => {
                    report.n_player_games_early += 1;
                    for k in 0..7 {
                        report.civil_spend_early[k].merge(tracks[i as usize].civil_spend_by_round[k]);
                    }
                    if let Some(s) = tracks[i as usize].round6_sample {
                        report.round6_early.push(s);
                    }
                }
                WonderTempoGroup::Late => {
                    report.n_player_games_late += 1;
                    for k in 0..7 {
                        report.civil_spend_late[k].merge(tracks[i as usize].civil_spend_by_round[k]);
                    }
                    if let Some(s) = tracks[i as usize].round6_sample {
                        report.round6_late.push(s);
                    }
                }
            }
        }

        // ---- card fate EARLY/LATE cross-tab: same grouping as the
        // wonder-tempo block just above. "Still in hand at game end" and
        // its censored dwell are resolved HERE, from whatever is left in
        // `taken_rounds` after every PLAYED/ANTIQUATED pop during the move
        // loop -- the same "whatever never resolved during play is
        // resolved at game end" shape `WonderFate::StillInProgress` uses.
        if let Some(group) = wonder_tempo_group(tracks[i as usize].first_wonder_round) {
            let mut still_in_hand: u64 = 0;
            let mut censored_dwell_this_game: Vec<i32> = tracks[i as usize].censored_dwell.clone();
            // "Never played" playable-turns pool starts from the ANTIQUATED
            // half (recorded per-entry at its own pop site, above) and gets
            // the StillInHand half appended right here, matching exactly
            // how `censored_dwell_this_game` above blends the same two
            // populations.
            let mut playable_turns_never_played_this_game: Vec<u32> =
                tracks[i as usize].playable_turns_antiquated.clone();
            // Same blend as `playable_turns_never_played_this_game` above,
            // for the poverty/selection-loss buckets -- ANTIQUATED half
            // recorded at its own pop site, StillInHand half appended here,
            // both restricted to zero-playable-turns entries only (see the
            // `if playable_turns == 0` guard at the ANTIQUATED pop site).
            let mut blocked_never_played_zero_this_game: Vec<(u32, u32, u32)> =
                tracks[i as usize].blocked_zero_antiquated.clone();
            for queue in tracks[i as usize].taken_rounds.values() {
                for entry in queue {
                    still_in_hand += 1;
                    censored_dwell_this_game.push(final_round as i32 - entry.taken_round as i32);
                    playable_turns_never_played_this_game.push(entry.playable_turns);
                    if entry.playable_turns == 0 {
                        blocked_never_played_zero_this_game
                            .push((entry.blocked_no_civil_action, entry.blocked_nothing_affordable, entry.blocked_something_else_developable));
                    }
                }
            }
            let n_taken = tracks[i as usize].n_taken;
            let n_played = tracks[i as usize].n_played;
            let n_antiquated = tracks[i as usize].n_antiquated;
            let played_dwell_this_game = tracks[i as usize].played_dwell.clone();
            let playable_turns_played_this_game = tracks[i as usize].playable_turns_played.clone();
            let blocked_played_this_game = tracks[i as usize].blocked_played.clone();
            match group {
                WonderTempoGroup::Early => {
                    report.card_fate_early.record(CardFate::Played, n_played as u64);
                    report.card_fate_early.record(CardFate::Antiquated, n_antiquated as u64);
                    report.card_fate_early.record(CardFate::StillInHand, still_in_hand);
                    report.cards_taken_per_playergame_early.push(n_taken as i32);
                    report.cards_still_in_hand_per_playergame_early.push(still_in_hand as i32);
                    report.played_dwell_early.extend(played_dwell_this_game);
                    report.censored_dwell_early.extend(censored_dwell_this_game);
                    report.playable_turns_played_early.extend(playable_turns_played_this_game);
                    report.playable_turns_never_played_early.extend(playable_turns_never_played_this_game);
                    report.blocked_played_early.extend(blocked_played_this_game);
                    report.blocked_never_played_zero_early.extend(blocked_never_played_zero_this_game);
                }
                WonderTempoGroup::Late => {
                    report.card_fate_late.record(CardFate::Played, n_played as u64);
                    report.card_fate_late.record(CardFate::Antiquated, n_antiquated as u64);
                    report.card_fate_late.record(CardFate::StillInHand, still_in_hand);
                    report.cards_taken_per_playergame_late.push(n_taken as i32);
                    report.cards_still_in_hand_per_playergame_late.push(still_in_hand as i32);
                    report.played_dwell_late.extend(played_dwell_this_game);
                    report.censored_dwell_late.extend(censored_dwell_this_game);
                    report.playable_turns_played_late.extend(playable_turns_played_this_game);
                    report.playable_turns_never_played_late.extend(playable_turns_never_played_this_game);
                    report.blocked_played_late.extend(blocked_played_this_game);
                    report.blocked_never_played_zero_late.extend(blocked_never_played_zero_this_game);
                }
            }
        }
        report.n_card_fate_mismatches += tracks[i as usize].n_card_fate_mismatch as u64;

        let completed: Vec<CardId> = p.completed_wonders.as_slice().to_vec();
        let started_count = tracks[i as usize].wonder_started.len();
        report.wonders_completed_per_playergame.push(completed.len() as i32);
        report.wonders_started_per_playergame.push(started_count as i32);
        report
            .wonders_abandoned_per_playergame
            .push((started_count as i32 - completed.len() as i32).max(0));
        for c in &completed {
            *report.wonder_completed_name_counts.entry(c.name()).or_insert(0) += 1;
        }

        // ---- wonder fate: resolve whatever is still in the slot, then
        // tally every distinct wonder this player-game's slot ever held
        // into exactly one WonderFate bucket.
        if !p.wonder.is_none() {
            tracks[i as usize].wonder_fate.entry(p.wonder).or_insert(WonderFate::StillInProgress);
        }
        let wonder_started_keys: Vec<CardId> = tracks[i as usize].wonder_started.keys().copied().collect();
        for card in &wonder_started_keys {
            match tracks[i as usize].wonder_fate.get(card) {
                Some(&fate) => report.wonder_fate.record(fate),
                // Left the slot, or never resolved above, but has no
                // recorded fate: the move loop's classification missed it.
                None => report.n_wonder_fate_mismatches += 1,
            }
        }

        *report.final_government.entry(p.government.name()).or_insert(0) += 1;
        report.gov_change_count_per_playergame.push(tracks[i as usize].gov_changes as i32);
        if tracks[i as usize].gov_changes == 0 {
            report.n_stayed_despotism += 1;
        }
        if let Some(r) = tracks[i as usize].first_gov_change_round {
            report.first_gov_change_round.push(r as i32);
        }

        report.wars_declared_per_playergame.push(tracks[i as usize].wars_declared as i32);
        report.aggressions_played_per_playergame.push(tracks[i as usize].aggressions as i32);

        report.final_score.push(p.culture as i32);
    }

    // weakest-at-end-of-age-II: compare strengths within the age-II bucket
    // samples just recorded for THIS game only (players.len() consecutive
    // entries at the tail of age_samples[2], if that boundary was reached).
    let age2_idx = age_index(Age::II);
    let len = report.age_samples[age2_idx].len();
    if len >= players as usize {
        let this_game = &report.age_samples[age2_idx][len - players as usize..];
        let min_strength = this_game.iter().map(|s| s.strength).min().unwrap();
        let n_at_min = this_game.iter().filter(|s| s.strength == min_strength).count();
        if n_at_min == 1 {
            report.n_weakest_at_end_age2 += 1; // exactly one strict-minimum player
        }
        report.n_playergames_with_age2_sample += players as u64;
    }

    report.games = 1;
    (report, cap_hit)
}

// ---------------------------------------------------------------------
// CLI
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
usage: behavcensus --weights PATH [options]

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

fn top_n(map: &HashMap<&'static str, u64>, n: usize) -> Vec<(&'static str, u64)> {
    let mut v: Vec<(&'static str, u64)> = map.iter().map(|(&k, &c)| (k, c)).collect();
    v.sort_unstable_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.truncate(n);
    v
}

/// [`top_n`]'s twin for the `opening_*` (wins, total) maps -- ranked by
/// TOTAL (frequency), not by win-rate, matching every other ranking in this
/// file (a rare, high-win-rate line should not out-rank a common,
/// average-win-rate one in a "most common opening lines" table).
fn top_pair_n(map: &HashMap<&'static str, (u64, u64)>, n: usize) -> Vec<(&'static str, (u64, u64))> {
    let mut v: Vec<(&'static str, (u64, u64))> = map.iter().map(|(&k, &c)| (k, c)).collect();
    v.sort_unstable_by(|a, b| b.1 .1.cmp(&a.1 .1).then(a.0.cmp(b.0)));
    v.truncate(n);
    v
}

/// Reconstructs the flat per-player-game round sample a `(round -> (wins,
/// total))` map was built from, so [`percentiles_i32`] can still print the
/// same distribution it always has, now sourced from `Report::
/// first_wonder_round`'s win-rate map instead of a bare `Vec<i32>`.
fn flatten_round_totals(map: &HashMap<i32, (u64, u64)>) -> Vec<i32> {
    let mut v = Vec::new();
    for (&round, &(_, total)) in map {
        v.extend(std::iter::repeat_n(round, total as usize));
    }
    v
}

fn print_report(players: u8, r: &Report) {
    println!("\n## {players}p (n={} games, {} player-games)\n", r.games, r.n_player_games);

    println!("### Openings\n");
    println!("First card taken while in Age A (top 10 by frequency, {} player-games):", r.n_player_games);
    for (name, count) in top_n(&r.first_take_card, 10) {
        println!("- {name}: {count} ({:.1}%)", 100.0 * count as f64 / r.n_player_games.max(1) as f64);
    }
    println!("\nFirst technology developed (top 10 by frequency):");
    for (name, count) in top_n(&r.first_develop_card, 10) {
        println!("- {name}: {count} ({:.1}%)", 100.0 * count as f64 / r.n_player_games.max(1) as f64);
    }
    println!("\nRound of first Develop: {}", percentiles_i32(r.first_develop_round.clone()));
    println!(
        "Round of first wonder-stage build (only among player-games that ever built one): {}",
        percentiles_i32(flatten_round_totals(&r.first_wonder_round))
    );
    println!("\nRound of first wonder-stage build, count/share/win-rate (outcome-resolvable player-games only):");
    let mut wonder_round_keys: Vec<i32> = r.first_wonder_round.keys().copied().collect();
    wonder_round_keys.sort_unstable();
    for k in wonder_round_keys {
        let (w, t) = r.first_wonder_round[&k];
        println!(
            "- round {k}: {t} ({:.1}%)  win-rate {:.1}% ({w}/{t})",
            100.0 * t as f64 / r.n_player_games.max(1) as f64,
            100.0 * w as f64 / t.max(1) as f64
        );
    }
    let (never_w, never_t) = r.first_wonder_never_built;
    println!(
        "- never built a wonder stage at all: {never_t} ({:.1}%)  win-rate {:.1}% ({never_w}/{never_t})",
        100.0 * never_t as f64 / r.n_player_games.max(1) as f64,
        100.0 * never_w as f64 / never_t.max(1) as f64
    );
    println!(
        "\nPlayer-games with NO wonder step by end of round 4: {}/{} ({:.1}%)",
        r.n_player_games_no_wonder_by_round4,
        r.n_player_games,
        100.0 * r.n_player_games_no_wonder_by_round4 as f64 / r.n_player_games.max(1) as f64
    );

    // ---- opening, human-comparable schema -- directly diffable against
    // `bin/humanopenings.rs`'s TSV, aggregated the same way: count, share
    // of player-games, and win-rate with its own sample size (only
    // player-games with a resolvable winner -- see `is_winner` in
    // `play_one` -- count toward a win-rate; `opening_outcome_unknown`
    // reports how many did not).
    println!(
        "\n### Opening (human-comparable schema: first take name+build kind+leader/pop by round 3)\n"
    );
    println!(
        "Outcome unknown (cap_hit or tie, excluded from every win-rate below): {}/{} ({:.1}%)",
        r.opening_outcome_unknown,
        r.n_player_games,
        100.0 * r.opening_outcome_unknown as f64 / r.n_player_games.max(1) as f64
    );
    println!("\nFirst card taken by round 3 (top 15), count/share/win-rate:");
    for (name, (w, t)) in top_pair_n(&r.opening_first_take, 15) {
        println!(
            "- {name}: {t} ({:.1}%)  win-rate {:.1}% ({w}/{t})",
            100.0 * t as f64 / r.n_player_games.max(1) as f64,
            100.0 * w as f64 / t.max(1) as f64
        );
    }
    println!("\nFirst build/develop/upgrade kind by round 3, count/share/win-rate:");
    for (name, (w, t)) in top_pair_n(&r.opening_first_build_kind, 15) {
        println!(
            "- {name}: {t} ({:.1}%)  win-rate {:.1}% ({w}/{t})",
            100.0 * t as f64 / r.n_player_games.max(1) as f64,
            100.0 * w as f64 / t.max(1) as f64
        );
    }
    for (label, map) in [("Elected >=1 leader by round 3", &r.opening_leader_r3), ("Increased population by round 3", &r.opening_pop_r3)] {
        for val in [true, false] {
            let (w, t) = map.get(&val).copied().unwrap_or((0, 0));
            println!(
                "{label} = {val}: {t} ({:.1}%)  win-rate {:.1}% ({w}/{t})",
                100.0 * t as f64 / r.n_player_games.max(1) as f64,
                100.0 * w as f64 / t.max(1) as f64
            );
        }
    }
    let mut ca_keys: Vec<i32> = r.opening_ca_unused.keys().copied().collect();
    ca_keys.sort_unstable();
    print!("\nCivil actions left unused across rounds 1-3 (count -> player-games): ");
    for k in ca_keys {
        print!("{k}->{} ", r.opening_ca_unused[&k]);
    }
    println!();

    let mut cost_keys: Vec<i32> = r.opening_first_take_cost.keys().copied().collect();
    cost_keys.sort_unstable();
    let cost_total: u64 = r.opening_first_take_cost.values().sum();
    print!("First Age-A take, CA price paid (price -> player-games): ");
    for k in cost_keys {
        let n = r.opening_first_take_cost[&k];
        let pct = if cost_total == 0 { 0.0 } else { 100.0 * n as f64 / cost_total as f64 };
        print!("{k}->{n} ({pct:.1}%) ");
    }
    println!("[n={cost_total}]");

    println!("\n### Wonders\n");
    println!("Wonders completed per player-game: {}", percentiles_i32(r.wonders_completed_per_playergame.clone()));
    println!("Wonders started per player-game: {}", percentiles_i32(r.wonders_started_per_playergame.clone()));
    println!(
        "Wonders started-but-abandoned per player-game: {}",
        percentiles_i32(r.wonders_abandoned_per_playergame.clone())
    );
    let mut dist: HashMap<i32, u64> = HashMap::new();
    for &c in &r.wonders_completed_per_playergame {
        *dist.entry(c).or_insert(0) += 1;
    }
    let mut keys: Vec<i32> = dist.keys().copied().collect();
    keys.sort_unstable();
    print!("Distribution of wonders completed (count -> player-games): ");
    for k in keys {
        print!("{k}->{} ", dist[&k]);
    }
    println!();
    println!("\nWonders completed by name (most to least, top 16):");
    for (name, count) in top_n(&r.wonder_completed_name_counts, 16) {
        println!("- {name}: {count}");
    }

    println!("\nFate of every distinct wonder that ever occupied a `wonder` slot ({} total):", r.wonder_fate.total());
    let f = &r.wonder_fate;
    let pct = |n: u64| 100.0 * n as f64 / f.total().max(1) as f64;
    println!("- Completed: {} ({:.1}%)", f.completed, pct(f.completed));
    println!("- Destroyed by Infiltrate: {} ({:.1}%)", f.infiltrated, pct(f.infiltrated));
    println!("- Destroyed by antiquation (\u{a7}12.2, age-end): {} ({:.1}%)", f.antiquated, pct(f.antiquated));
    println!(
        "- Destroyed, unexplained (engine cleared `.wonder` from a fourth site the rulebook does not \
         sanction -- a real bug, not a census gap; see WonderFate::DestroyedUnexplained's doc comment): {} ({:.1}%)",
        f.unexplained,
        pct(f.unexplained)
    );
    println!("- Still in progress at game end: {} ({:.1}%)", f.still_in_progress, pct(f.still_in_progress));
    if r.n_wonder_fate_mismatches > 0 {
        println!(
            "WARNING  {} wonder(s) started but never resolved to a fate -- bucket totals above do NOT \
             account for every wonder the census saw; the classification in play_one() has a gap",
            r.n_wonder_fate_mismatches
        );
    }

    println!("\n### Government\n");
    println!("Final government (all player-games):");
    for (name, count) in top_n(&r.final_government, 8) {
        println!("- {name}: {count} ({:.1}%)", 100.0 * count as f64 / r.n_player_games.max(1) as f64);
    }
    println!(
        "\nStayed in Despotism the entire game: {}/{} ({:.1}%)",
        r.n_stayed_despotism,
        r.n_player_games,
        100.0 * r.n_stayed_despotism as f64 / r.n_player_games.max(1) as f64
    );
    println!("Government changes per player-game: {}", percentiles_i32(r.gov_change_count_per_playergame.clone()));
    println!(
        "Round of first government change (only player-games with >=1 change): {}",
        percentiles_i32(r.first_gov_change_round.clone())
    );

    println!("\n### Military\n");
    println!("Wars declared per player-game: {}", percentiles_i32(r.wars_declared_per_playergame.clone()));
    println!("Aggressions played per player-game: {}", percentiles_i32(r.aggressions_played_per_playergame.clone()));
    if r.n_playergames_with_age2_sample > 0 {
        println!(
            "Strict-weakest military strength at end of Age II: {}/{} player-games with a sample ({:.1}%; \
             baseline under identical self-play is 1/players, so deviation from that flags asymmetric outcomes \
             even among identical weights, e.g. seating order)",
            r.n_weakest_at_end_age2,
            r.n_playergames_with_age2_sample,
            100.0 * r.n_weakest_at_end_age2 as f64 / r.n_playergames_with_age2_sample as f64
        );
    } else {
        println!("No Age II boundary reached in this sample (game(s) too short) -- N/A");
    }

    println!("\n### Economy / culture / science by age\n");
    println!("| age boundary | farm workers | mine workers | lab workers | food stock | resource stock | military strength | culture/turn | science/turn |");
    println!("|---|---|---|---|---|---|---|---|---|");
    for i in 0..5 {
        let s = &r.age_samples[i];
        if s.is_empty() {
            println!("| {} | (no samples) | | | | | | | |", age_label(i));
            continue;
        }
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            age_label(i),
            percentiles_u32(s.iter().map(|x| x.farm_workers).collect()),
            percentiles_u32(s.iter().map(|x| x.mine_workers).collect()),
            percentiles_u32(s.iter().map(|x| x.lab_workers).collect()),
            percentiles_u32(s.iter().map(|x| x.food).collect()),
            percentiles_u32(s.iter().map(|x| x.resources).collect()),
            percentiles_i32(s.iter().map(|x| x.strength).collect()),
            percentiles_i32(s.iter().map(|x| x.culture_rate).collect()),
            percentiles_i32(s.iter().map(|x| x.science_rate).collect()),
        );
    }
    // "starved" proxy: no explicit starvation flag exists in the engine
    // (see the module doc) -- reported as a stock-at-or-near-zero proxy.
    for i in 0..5 {
        let s = &r.age_samples[i];
        if s.is_empty() {
            continue;
        }
        let n_food_zero = s.iter().filter(|x| x.food == 0).count();
        let n_res_zero = s.iter().filter(|x| x.resources == 0).count();
        println!(
            "  proxy at {}: food stock == 0 in {}/{} ({:.1}%); resource stock == 0 in {}/{} ({:.1}%)",
            age_label(i),
            n_food_zero,
            s.len(),
            100.0 * n_food_zero as f64 / s.len() as f64,
            n_res_zero,
            s.len(),
            100.0 * n_res_zero as f64 / s.len() as f64
        );
    }

    println!("\n### Score\n");
    println!("Final score (= culture stock, the engine's only scoring quantity): {}", percentiles_i32(r.final_score.clone()));
    println!(
        "\nNo per-source score decomposition (wonders/culture buildings/leaders/etc.) is available: \
         `game::scores` returns `PlayerState::culture`, a single accumulating stock with no source-tagged \
         ledger anywhere in the engine -- see this file's module doc."
    );

    // ---- wonder tempo: what did the LATE group's civil actions go toward
    // instead of a wonder step, and how far behind is its board by round 6?
    // See the "Wonder-tempo EARLY/LATE grouping" section near the top of
    // this file and analysis/wonder_tempo_2026-08-24.txt for why this
    // comparison exists and what it measured before this instrumentation.
    println!("\n### Wonder tempo: civil-action spend by move kind, rounds 3-9, EARLY vs LATE\n");
    println!(
        "EARLY = first wonder-stage build by round 5 (n={} player-games). LATE = round 10+ or \
         never built (n={} player-games). Player-games whose first wonder-stage round is 6-9 are \
         in neither group (see analysis/wonder_tempo_2026-08-24.txt).",
        r.n_player_games_early, r.n_player_games_late
    );
    println!("\n| round | group | n (CA spent) | take | build | develop | pop | leader | action-card | wonder-step | other |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for round in 3u16..=9u16 {
        let idx = (round - 3) as usize;
        for (label, counts) in [("EARLY", &r.civil_spend_early[idx]), ("LATE", &r.civil_spend_late[idx])] {
            let total = counts.total();
            let share = |n: u64| 100.0 * n as f64 / total.max(1) as f64;
            println!(
                "| {round} | {label} | {total} | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% | {:.1}% |",
                share(counts.take),
                share(counts.build),
                share(counts.develop),
                share(counts.pop),
                share(counts.leader),
                share(counts.action_card),
                share(counts.wonder_step),
                share(counts.other),
            );
        }
    }

    println!("\n### Wonder tempo: end-of-round-6 board state, EARLY vs LATE\n");
    for (label, samples) in [("EARLY", &r.round6_early), ("LATE", &r.round6_late)] {
        println!("\n{label} (n={}):", samples.len());
        println!("  total culture:       {}", percentiles_u32(samples.iter().map(|s| s.culture_stock).collect()));
        println!("  science production:  {}", percentiles_i32(samples.iter().map(|s| s.science_rate).collect()));
        println!("  food production:     {}", percentiles_i32(samples.iter().map(|s| s.food_rate).collect()));
        println!("  resource production: {}", percentiles_i32(samples.iter().map(|s| s.resource_rate).collect()));
        println!("  military strength:   {}", percentiles_i32(samples.iter().map(|s| s.strength).collect()));
        println!("  cards in civil hand: {}", percentiles_u32(samples.iter().map(|s| s.hand_civil).collect()));
        println!("  buildings on board:  {}", percentiles_u32(samples.iter().map(|s| s.buildings).collect()));
    }

    // ---- card fate: are LATE's extra taken civil cards WASTED, or a real
    // investment that pays off later? See the "Card fate" section near
    // CivilMoveKind above for the fate classification and its detection.
    println!("\n### Card fate: are LATE's extra taken civil cards wasted, or a real investment paying off later?\n");
    println!(
        "Same EARLY/LATE grouping as the wonder-tempo tables above (see analysis/wonder_tempo_2026-08-24.txt). \
         PLAYED = left hand_civil via Develop/PlayLeader/Revolution/PlayAction (see played_civil_card's doc \
         comment) -- Move::Build/Move::Upgrade never touch hand_civil at all, so they cannot be this event. \
         ANTIQUATED = culled from hand at an age transition (RULES_SPEC.md \u{a7}12.2). There is no separate \
         hand-LIMIT discard event in this engine to measure -- civil_hand_limit only blocks an illegal Take \
         (game::force_civil_age_at_least's doc comment), it never forces a discard -- so item 4's hand-limit \
         half is not reported because it does not exist, not because it was skipped."
    );
    if r.n_card_fate_mismatches > 0 {
        println!(
            "WARNING  {} card(s) played or antiquated with no matching taken_rounds entry -- a real bug in \
             this census's own tracking, not a sample gap (see Report::n_card_fate_mismatches's doc comment)",
            r.n_card_fate_mismatches
        );
    }
    for (label, counts, taken_v, hand_v, played_dwell_v, censored_dwell_v, n_pg) in [
        (
            "EARLY",
            &r.card_fate_early,
            &r.cards_taken_per_playergame_early,
            &r.cards_still_in_hand_per_playergame_early,
            &r.played_dwell_early,
            &r.censored_dwell_early,
            r.n_player_games_early,
        ),
        (
            "LATE",
            &r.card_fate_late,
            &r.cards_taken_per_playergame_late,
            &r.cards_still_in_hand_per_playergame_late,
            &r.played_dwell_late,
            &r.censored_dwell_late,
            r.n_player_games_late,
        ),
    ] {
        println!("\n{label} (n={n_pg} player-games):");
        let total = counts.total();
        let pct = |n: u64| 100.0 * n as f64 / total.max(1) as f64;
        println!("  civil cards taken over the whole game: {}", percentiles_i32(taken_v.clone()));
        println!(
            "  fate of every taken card ({total} total): played {} ({:.1}%), antiquated {} ({:.1}%), \
             still in hand at game end {} ({:.1}%)",
            counts.played,
            pct(counts.played),
            counts.antiquated,
            pct(counts.antiquated),
            counts.still_in_hand,
            pct(counts.still_in_hand)
        );
        println!("  cards still in hand at game end:       {}", percentiles_i32(hand_v.clone()));
        println!("  dwell (rounds in hand), PLAYED cards:              {}", percentiles_i32(played_dwell_v.clone()));
        println!("  dwell (rounds in hand), NEVER played (censored):   {}", percentiles_i32(censored_dwell_v.clone()));
    }

    // ---- card fate follow-up: WHY does a never-played card rot -- never
    // legal again after the take, or legal but losing the CA auction every
    // turn? See the "Card fate follow-up" section above `TakenCard`'s own
    // definition for the `legal::legal_moves` check this counts.
    println!("\n### Card fate follow-up: was a never-played card ever LEGAL to play?\n");
    println!(
        "playable-turns = count of this player's own decision points, since the card was taken, at which \
         Develop/PlayLeader/Revolution/PlayAction of it appeared in legal::legal_moves (the same move list \
         Seat::pick already chose from) -- ZERO means the card was never legal again after the take (a bad \
         take, or the resources/prereqs never arrived); nonzero means it lost the civil-action auction to \
         something else on every turn it was legal, until it was culled or the game ended. PLAYED cards are \
         the control population: how many turns a card sat legal before it was actually played."
    );
    for (label, played_v, never_v) in [
        ("EARLY", &r.playable_turns_played_early, &r.playable_turns_never_played_early),
        ("LATE", &r.playable_turns_played_late, &r.playable_turns_never_played_late),
    ] {
        let n_never = never_v.len();
        let n_never_zero = never_v.iter().filter(|&&n| n == 0).count();
        let zero_share = 100.0 * n_never_zero as f64 / (n_never.max(1)) as f64;
        println!("\n{label}:");
        println!("  playable-turns, PLAYED cards (control):   {}", percentiles_u32(played_v.clone()));
        println!("  playable-turns, NEVER-played cards:       {}", percentiles_u32(never_v.clone()));
        println!(
            "  of {n_never} never-played cards, {n_never_zero} ({zero_share:.1}%) were NEVER legal to play again \
             after the take -- the rest ({}, {:.1}%) were legal on at least one later turn and lost the CA \
             auction to something else",
            n_never - n_never_zero,
            100.0 - zero_share
        );
    }

    // ---- zero-playable-turns follow-up: WHY was a never-played,
    // zero-playable-turns card never legal -- was the whole hand too poor
    // to develop anything, or did some OTHER card in hand win every time?
    // Read off the SAME `legal_now` MoveList the playable-turns count
    // above already computed (see `TakenCard::blocked_no_civil_action`,
    // `blocked_nothing_affordable` and `blocked_something_else_developable`'s
    // doc comments), never a second `legal::legal_moves` call.
    println!("\n### Zero playable-turns follow-up: why was it never legal?\n");
    println!(
        "For never-played cards that scored zero playable-turns, every one of their blocked decision points is \
         attributed to exactly one of three buckets: \"no civil action\" (nothing in hand was legal because the \
         player had no action left to spend -- an action-budget miss, the hand may have been affordable), \
         \"nothing affordable\" (an action WAS available and still nothing in hand could be paid for -- a true \
         production shortfall), or \"something else developable\" (some OTHER civil card in hand WAS legal, so \
         this specific card lost the competition). PLAYED cards are the control: the same three buckets, over \
         however many blocked turns they sat through before finally winning one."
    );
    println!(
        "The \"at least one turn\" shares below are counts of CARDS and inflate easily over a long dwell; the \
         per-turn shares are the comparable quantity, because civil-action exhaustion hits both populations at \
         the same rate and cancels out of a ratio."
    );
    for (label, played_v, never_zero_v) in [
        ("EARLY", &r.blocked_played_early, &r.blocked_never_played_zero_early),
        ("LATE", &r.blocked_played_late, &r.blocked_never_played_zero_late),
    ] {
        let n_never_zero = never_zero_v.len();
        let n_pure_poverty = never_zero_v.iter().filter(|&&(_, _, s)| s == 0).count();
        let n_selection_loss = n_never_zero - n_pure_poverty;
        let n_no_blocked_turns = never_zero_v.iter().filter(|&&(a, p, s)| a == 0 && p == 0 && s == 0).count();
        let pure_poverty_share = 100.0 * n_pure_poverty as f64 / (n_never_zero.max(1)) as f64;
        let selection_loss_share = 100.0 * n_selection_loss as f64 / (n_never_zero.max(1)) as f64;
        let no_blocked_share = 100.0 * n_no_blocked_turns as f64 / (n_never_zero.max(1)) as f64;
        println!("\n{label}:");
        for (who, v) in [
            ("PLAYED cards (control)          ", played_v),
            ("NEVER-played zero-playable-turn ", never_zero_v),
        ] {
            let no_action: Vec<u32> = v.iter().map(|&(a, _, _)| a).collect();
            let nothing_afford: Vec<u32> = v.iter().map(|&(_, p, _)| p).collect();
            let something_else: Vec<u32> = v.iter().map(|&(_, _, s)| s).collect();
            let sum_action: u64 = no_action.iter().map(|&x| u64::from(x)).sum();
            let sum_afford: u64 = nothing_afford.iter().map(|&x| u64::from(x)).sum();
            let sum_else: u64 = something_else.iter().map(|&x| u64::from(x)).sum();
            let total = (sum_action + sum_afford + sum_else).max(1) as f64;
            println!("  {who} blocked \"no civil action\":         {}", percentiles_u32(no_action));
            println!("  {who} blocked \"nothing affordable\":      {}", percentiles_u32(nothing_afford));
            println!("  {who} blocked \"something else developable\": {}", percentiles_u32(something_else));
            println!(
                "  {who} per-turn share of ALL its blocked decision points: no civil action {:.1}%, \
                 nothing affordable {:.1}%, something else developable {:.1}%",
                100.0 * sum_action as f64 / total,
                100.0 * sum_afford as f64 / total,
                100.0 * sum_else as f64 / total
            );
        }
        println!(
            "  of {n_never_zero} never-played zero-playable-turn cards: {n_pure_poverty} ({pure_poverty_share:.1}%) \
             never once shared a decision point with a developable card, {n_selection_loss} \
             ({selection_loss_share:.1}%) had at least one turn where something ELSE in hand was developable, \
             and {n_no_blocked_turns} ({no_blocked_share:.1}%) sat through zero decision points of their own \
             before resolving"
        );
    }

    // ---- production curve: mean worker-capped food/resource production
    // (`economy::production_this_turn`) per round, one sample per
    // player-round at the START of that player's turn -- directly
    // comparable against `bin/humanopenings.rs`'s identically-formatted
    // human curve (same print format, same sampling instant).
    println!("\n### Production curve\n");
    let mut production_rounds: Vec<u16> = r.production_by_round.keys().copied().collect();
    production_rounds.sort_unstable();
    for round in production_rounds {
        let (food_sum, resources_sum, n) = r.production_by_round[&round];
        println!(
            "round {round}: food mean={:.2} resources mean={:.2} n={n}",
            food_sum as f64 / n.max(1) as f64,
            resources_sum as f64 / n.max(1) as f64
        );
    }

    // ---- worker allocation curve: mean workers by CardType bucket, mean
    // free/staffed workers, and mean of each player's OWN best farm/mine
    // tech level, per round -- same sample instant as the production curve
    // above (see `AllocAccum`'s doc), same print format as
    // `bin/humanopenings.rs`'s so the two are directly diffable.
    println!("\n### Worker allocation curve\n");
    let mut alloc_rounds: Vec<u16> = r.alloc_by_round.keys().copied().collect();
    alloc_rounds.sort_unstable();
    for round in alloc_rounds {
        let a = &r.alloc_by_round[&round];
        let n = a.n.max(1) as f64;
        println!(
            "round {round}: farmW={:.2} mineW={:.2} urbanW={:.2} milW={:.2} free={:.2} staffed={:.2} bestFarm={:.2} bestMine={:.2} n={}",
            a.farm_workers as f64 / n,
            a.mine_workers as f64 / n,
            a.urban_workers as f64 / n,
            a.mil_workers as f64 / n,
            a.free_workers as f64 / n,
            a.staffed_workers as f64 / n,
            a.best_farm_sum as f64 / n,
            a.best_mine_sum as f64 / n,
            a.n
        );
    }

    // ---- tech acquisition: seen -> taken -> built -> staffed, per
    // `TechKind` bucket, one sample per player-game (see the "Tech
    // acquisition" section's doc comment above `TechKind` for exactly what
    // each stage measures and which existing hook it is read from). Farm/
    // Mine additionally broken down by age tier for TAKEN and BUILT -- the
    // question this section exists to answer is whether the bot ever
    // acquires a HIGHER-tier production tech than the starting Bronze/
    // Agriculture.
    println!("\n### Tech acquisition\n");
    let tacq_n = r.n_player_games.max(1) as f64;
    println!("per player-game (n={}):", r.n_player_games);
    println!("{:<10} {:>8} {:>8} {:>8} {:>8}", "type", "seen", "taken", "built", "staffed");
    for kind in ALL_TECH_KINDS {
        let c = r.tech_acq.get(&kind).copied().unwrap_or_default();
        println!(
            "{:<10} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
            tech_kind_label(kind),
            c.seen as f64 / tacq_n,
            c.taken as f64 / tacq_n,
            c.built as f64 / tacq_n,
            c.staffed as f64 / tacq_n,
        );
    }
    println!("\nFarm/Mine taken by age tier, per player-game:");
    println!(
        "  Farm: A={:.3} I={:.3} II={:.3} III={:.3}",
        r.farm_tier.taken[0] as f64 / tacq_n,
        r.farm_tier.taken[1] as f64 / tacq_n,
        r.farm_tier.taken[2] as f64 / tacq_n,
        r.farm_tier.taken[3] as f64 / tacq_n,
    );
    println!(
        "  Mine: A={:.3} I={:.3} II={:.3} III={:.3}",
        r.mine_tier.taken[0] as f64 / tacq_n,
        r.mine_tier.taken[1] as f64 / tacq_n,
        r.mine_tier.taken[2] as f64 / tacq_n,
        r.mine_tier.taken[3] as f64 / tacq_n,
    );
    println!("Farm/Mine built by age tier, per player-game:");
    println!(
        "  Farm: A={:.3} I={:.3} II={:.3} III={:.3}",
        r.farm_tier.built[0] as f64 / tacq_n,
        r.farm_tier.built[1] as f64 / tacq_n,
        r.farm_tier.built[2] as f64 / tacq_n,
        r.farm_tier.built[3] as f64 / tacq_n,
    );
    println!(
        "  Mine: A={:.3} I={:.3} II={:.3} III={:.3}",
        r.mine_tier.built[0] as f64 / tacq_n,
        r.mine_tier.built[1] as f64 / tacq_n,
        r.mine_tier.built[2] as f64 / tacq_n,
        r.mine_tier.built[3] as f64 / tacq_n,
    );
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("behavcensus: {e}");
            return ExitCode::FAILURE;
        }
    };

    let weights = match load_weights(std::path::Path::new(&args.weights_path)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("behavcensus: loading {}: {e}", args.weights_path);
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let mut results: Vec<Option<(Report, bool)>> = (0..args.games).map(|_| None).collect();

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
            for (i, r) in h.join().expect("behavcensus thread panicked") {
                slots[i] = Some(r);
            }
        }
    });

    let mut overall = Report::default();
    let mut capped = 0usize;
    for r in results {
        let (rep, cap) = r.expect("every game played");
        capped += usize::from(cap);
        overall.merge(rep);
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("games        {}", args.games);
    println!("players      {}", args.players);
    println!("weights      {}", args.weights_path);
    println!("seeds        {}..{}", args.seed, args.seed + args.games as u64 - 1);
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", args.games as f64 / elapsed.max(1e-9));
    if capped > 0 {
        println!("WARNING      {capped} game(s) hit the {MOVE_CAP}-move cap -- that is a bug, not a long game");
    }

    print_report(args.players, &overall);

    if capped > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn age_index_maps_every_age_to_a_distinct_slot_in_order() {
        assert_eq!(age_index(Age::A), 0);
        assert_eq!(age_index(Age::I), 1);
        assert_eq!(age_index(Age::II), 2);
        assert_eq!(age_index(Age::III), 3);
        assert_eq!(age_index(Age::IV), 4);
    }

    #[test]
    fn percentiles_i32_reports_min_and_max_at_the_ends_of_a_sorted_sample() {
        let s = percentiles_i32(vec![5, 1, 3, 2, 4]);
        assert!(s.contains("min=1"));
        assert!(s.contains("max=5"));
        assert!(s.contains("n=5"));
    }

    #[test]
    fn percentiles_i32_reports_n_a_for_an_empty_sample_rather_than_dividing_by_zero() {
        assert_eq!(percentiles_i32(vec![]), "n/a (no samples)");
    }

    #[test]
    fn top_n_ranks_by_descending_count_and_breaks_ties_alphabetically() {
        let mut m: HashMap<&'static str, u64> = HashMap::new();
        m.insert("Bronze", 3);
        m.insert("Alloy", 3);
        m.insert("Iron", 1);
        let ranked = top_n(&m, 2);
        assert_eq!(ranked, vec![("Alloy", 3), ("Bronze", 3)]);
    }

    #[test]
    fn a_two_player_self_play_game_plays_to_completion_and_records_at_least_one_age_sample() {
        let weights = Weights::default();
        let (report, cap_hit) = play_one(2, weights, 42);
        assert!(!cap_hit, "a 2p game should finish well inside the move cap");
        assert_eq!(report.games, 1);
        assert_eq!(report.n_player_games, 2);
        assert_eq!(report.final_score.len(), 2);
        // Every game reaches at least Age I, so that boundary's bucket
        // (index 0, "end of Age A") must be non-empty.
        assert!(!report.age_samples[0].is_empty(), "should have crossed at least one age boundary");
    }

    #[test]
    fn infiltrate_candidate_victim_flags_an_aggression_move_playing_a_remove_from_game_card() {
        // Case 1 of infiltrate_candidate_victim's doc comment: when the
        // defender has no military actions or an empty hand,
        // `interact::start_defense` resolves `finish_aggression` INLINE,
        // inside the very `Move::Aggression` step -- no `Pending::Defense`
        // is ever pushed for a later move to read. A fresh game has empty
        // `pending`, so this exercises exactly that no-decision-pending
        // path.
        let state = game::new_game(2, 1);
        assert!(state.pending.is_empty(), "a freshly started game has no open decision");
        let card = CardId::by_name("Aggression: Infiltrate").expect("card table has Aggression: Infiltrate");
        let mv = Move::Aggression { card, target: 1 };
        assert_eq!(infiltrate_candidate_victim(&state, mv), Some(1));
    }

    #[test]
    fn infiltrate_candidate_victim_ignores_aggression_cards_that_do_not_remove_anything() {
        // Not every Aggression card is Infiltrate-class (Special::
        // RemoveFromGame); a Raid/Annex/Plunder card must not be misread as
        // one, or every aggression against any player would get wrongly
        // credited as a wonder-destroyer.
        let state = game::new_game(2, 1);
        let plunder = CardId::by_name("Aggression: Plunder (III)").expect("card table has a non-Infiltrate aggression");
        let mv = Move::Aggression { card: plunder, target: 1 };
        assert_eq!(infiltrate_candidate_victim(&state, mv), None);
    }

    #[test]
    fn infiltrate_candidate_victim_reads_an_in_progress_defense_against_a_remove_from_game_card() {
        // Case 2: when the defender DOES have military cards to spend,
        // `interact::start_defense` pushes `Pending::Defense` and the
        // decision spans one or more `Move::Defend` / `Move::DefendDone`
        // moves before `interact::defense_move` pops it and calls
        // `finish_aggression`. The victim (`Defense.player`, the DEFENDER)
        // must be legible off `state.pending` for whichever of those moves
        // turns out to be the one that resolves it -- this is what lets the
        // census attribute the wonder clearing on THAT move rather than the
        // earlier `Move::Aggression` that only started the sequence.
        let mut state = game::new_game(2, 1);
        let card = CardId::by_name("Aggression: Infiltrate").expect("card table has Aggression: Infiltrate");
        state.pending.push(tta::state::Pending::Defense(tta::state::Defense {
            player: 1,
            attacker: 0,
            card,
            atk: 5,
            dfn: 2,
            spent: 0,
            budget: 1,
        }));
        // The move itself is irrelevant to this function -- only the top of
        // `state.pending` is read -- so `DefendDone` stands in for whichever
        // of the two legal moves the bot actually picked.
        assert_eq!(infiltrate_candidate_victim(&state, Move::DefendDone), Some(1));
    }

    #[test]
    fn infiltrate_candidate_victim_reads_an_open_infiltrate_choice() {
        // Case 3: a victim with BOTH a leader and a wonder gets a genuine
        // two-option decision (interact.rs's QueueItem::Infiltrate
        // handler), answered by a separate Move::Choose. The victim is
        // named on the `ChoiceKind::Infiltrate` itself, so this must not
        // require inspecting WHICH option index `n` was actually chosen --
        // the caller only acts on this if the wonder slot changed on this
        // exact move, which self-filters an answer of `Leader`.
        let mut state = game::new_game(2, 1);
        let mut opts = tta::state::OptionList::new();
        opts.push(tta::state::ChoiceOption::Word(tta::state::Keyword::Leader));
        opts.push(tta::state::ChoiceOption::Word(tta::state::Keyword::Wonder));
        state.pending.push(tta::state::Pending::Choice(tta::state::Choice {
            player: 0,
            kind: ChoiceKind::Infiltrate { victim: 1, per: 3 },
            options: opts,
        }));
        assert_eq!(infiltrate_candidate_victim(&state, Move::Choose { n: 1 }), Some(1));
    }

    #[test]
    fn classify_wonder_change_prefers_infiltrate_over_unexplained_for_the_auto_resolved_case() {
        // This is the regression test for the bug this change fixes: before
        // the structural detector, a wonder that vanished on an Aggression
        // move (not the age-change move, not a completion) with no matching
        // Move::Choose fell all the way through to DestroyedUnexplained. It
        // must now resolve to DestroyedByInfiltrate instead.
        let fate = classify_wonder_change(
            /* completed_this_move */ false,
            /* pending_infiltrate_victim */ Some(1),
            /* victim */ 1,
            /* age_changed_this_move */ false,
        );
        assert_eq!(fate, WonderFate::DestroyedByInfiltrate);
    }

    #[test]
    fn classify_wonder_change_falls_back_to_unexplained_only_when_nothing_else_accounts_for_the_clearing() {
        // Completion, Infiltrate and antiquation are the engine's only three
        // sites that clear `.wonder` (see WonderFate::DestroyedUnexplained's
        // doc comment) -- if none of those three signals fired, the bucket
        // genuinely means "the engine did something unsanctioned", which is
        // exactly what this case exercises.
        let fate = classify_wonder_change(false, None, 1, false);
        assert_eq!(fate, WonderFate::DestroyedUnexplained);
    }

    #[test]
    fn merge_combines_two_reports_games_and_score_vectors() {
        let mut a = Report { games: 1, ..Default::default() };
        a.final_score.push(10);
        let mut b = Report { games: 1, ..Default::default() };
        b.final_score.push(20);
        a.merge(b);
        assert_eq!(a.games, 2);
        assert_eq!(a.final_score, vec![10, 20]);
    }
}
