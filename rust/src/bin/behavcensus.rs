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

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Search, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::cards::Special;
use tta::effects;
use tta::game::{self, MOVE_CAP};
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

#[derive(Default)]
struct Report {
    games: u64,

    // ---- openings (one sample per player per game) ----
    first_take_card: HashMap<&'static str, u64>,
    first_develop_card: HashMap<&'static str, u64>,
    first_develop_round: Vec<i32>,
    first_wonder_round: Vec<i32>, // round of first WonderStep move, per player-game that ever built one
    n_player_games_no_wonder_by_round4: u64,
    n_player_games: u64,

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
}

impl Report {
    fn merge(&mut self, mut other: Report) {
        self.games += other.games;
        merge_map(&mut self.first_take_card, other.first_take_card);
        merge_map(&mut self.first_develop_card, other.first_develop_card);
        self.first_develop_round.extend(other.first_develop_round);
        self.first_wonder_round.extend(other.first_wonder_round);
        self.n_player_games_no_wonder_by_round4 += other.n_player_games_no_wonder_by_round4;
        self.n_player_games += other.n_player_games;

        merge_pair_map(&mut self.opening_first_take, other.opening_first_take);
        merge_pair_map(&mut self.opening_first_build_kind, other.opening_first_build_kind);
        merge_pair_map(&mut self.opening_leader_r3, other.opening_leader_r3);
        merge_pair_map(&mut self.opening_pop_r3, other.opening_pop_r3);
        merge_count_map(&mut self.opening_ca_unused, other.opening_ca_unused);
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
    }
}

fn merge_map(a: &mut HashMap<&'static str, u64>, b: HashMap<&'static str, u64>) {
    for (k, v) in b {
        *a.entry(k).or_insert(0) += v;
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

    while !state.game_over {
        if moves_played >= MOVE_CAP {
            cap_hit = true;
            break;
        }
        let mv = bots[state.current as usize].pick(&state);
        let actor = state.current;
        let round_before = state.round;

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

        // pre-move snapshot for every player, used only if this move is the
        // one that advances state.age_civil.
        let pre_snapshots: Vec<AgeSample> = (0..players).map(|i| sample_player(&state, i)).collect();

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

        // ---- openings ----
        if let Some(card) = taken_card {
            if !card.is_none() && pre_age == Age::A && tracks[actor as usize].first_take.is_none() {
                tracks[actor as usize].first_take = Some(card);
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
    }

    // ---- final / end-of-game bookkeeping ----
    let final_idx = age_index(Age::IV);
    let final_snapshots: Vec<AgeSample> = (0..players).map(|i| sample_player(&state, i)).collect();
    report.age_samples[final_idx].extend(final_snapshots);

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
        }

        if let Some(card) = tracks[i as usize].first_take {
            *report.first_take_card.entry(card.name()).or_insert(0) += 1;
        }
        if let Some((card, round)) = tracks[i as usize].first_develop {
            *report.first_develop_card.entry(card.name()).or_insert(0) += 1;
            report.first_develop_round.push(round as i32);
        }
        match tracks[i as usize].first_wonder_round {
            Some(r) => {
                report.first_wonder_round.push(r as i32);
                if r > 4 {
                    report.n_player_games_no_wonder_by_round4 += 1;
                }
            }
            None => report.n_player_games_no_wonder_by_round4 += 1,
        }

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
        percentiles_i32(r.first_wonder_round.clone())
    );
    println!(
        "Player-games with NO wonder step by end of round 4: {}/{} ({:.1}%)",
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
