//! `humanopenings` -- the human-corpus twin of `behavcensus.rs`'s opening
//! section, but sourced from REPLAYED human games (`replay_common::
//! replay_game`, `record_decisions: true`) instead of self-play, and scoped
//! to the opening only: each player's own moves from the start of the game
//! through the end of round 3 (Age A plus the first couple of Age I rounds
//! for most games), described the way a human would describe them --
//! per the "Canonical openings" measurement task.
//!
//! Covers every player count in the corpus (2p/3p/4p, 692/133/186 of 1011
//! games). Results are SEGMENTED by player count throughout -- an n-player
//! game is a structurally different game (card-row width, take-cost
//! economics, seat count all vary with `players`), so pooling counts into
//! one undifferentiated number is invalid; see the per-count loop and the
//! per-count cost-distribution summary below.
//!
//! ```text
//! cargo run --profile difftest --bin humanopenings -- \
//!     ../sources/bgo/index.tsv /tmp/bgo-journals/journals > openings.tsv
//! ```
//!
//! One TSV line per player-game on stdout (N lines per N-player game),
//! columns:
//! `game_id  seat  first_take_name  first_take_cost  first_build_kind
//!  first_build_name  leader_by_r3  pop_by_r3  ca_unused_by_r3  outcome`
//!
//! `outcome` is `win`/`loss`/`tie`/`unknown`, determined from the SAME
//! text-only "WINNER IS ... AS <COLOR> (<N> PTS); 2nd is ... as <Color>
//! (<n> pts)" parse `bin/humanwinners.rs` already uses for its own
//! winner/loser split (`corpus::parse_winner_line`, moved there from that
//! binary so both can call it) -- reused rather than gating outcome on full
//! engine-replay completion (`GameResult::engine_scores`, `Some` only for
//! the minority of games whose reconstruction reaches the literal end, see
//! `docs/CHAMPION_VS_HUMANS.md`'s "Completion" note); text coverage is far
//! higher. Seat->colour uses `corpus::Color::from_seat`, BGO's fixed
//! empirically-confirmed seating convention (Orange seat 0, Purple seat 1
//! for every 2p game) -- the same convention `replay_common`'s own
//! `target_actor_color` and `humanwinners::color_for_seat` already encode.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{self, parse_winner_line, Color};
use tta::replay_common::{build_card_index, replay_game};
use tta::{CardId, CardType, Move};

/// First-take civil-action-cost tally for one player count, accumulated
/// across every player-game at that count.
/// One player-round's contribution to the "Worker allocation curve" --
/// summed here, divided by `n` when printed. The human twin of
/// `bin/behavcensus.rs`'s `AllocAccum`, same fields, same sample instant
/// (see the accumulation site below), so the two curves are directly
/// diffable line-for-line.
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

#[derive(Default)]
struct CostStats {
    /// cost -> number of player-games whose first take had that cost.
    hist: BTreeMap<i32, u64>,
    sum: i64,
    n: u64,
}

impl CostStats {
    fn record(&mut self, cost: i32) {
        *self.hist.entry(cost).or_insert(0) += 1;
        self.sum += i64::from(cost);
        self.n += 1;
    }

    fn report(&self, players: u8) {
        if self.n == 0 {
            eprintln!("first-take cost, {players}p: n=0");
            return;
        }
        eprintln!("first-take cost, {players}p (n={}):", self.n);
        for (cost, count) in &self.hist {
            let pct = 100.0 * (*count as f64) / (self.n as f64);
            eprintln!("  cost {cost} = {count}/{} = {pct:.1}%", self.n);
        }
        let mean = (self.sum as f64) / (self.n as f64);
        eprintln!("  mean = {mean:.3}");
    }
}

/// min/p25/median/p75/max/mean of a non-empty sample, in `behavcensus.rs`'s
/// own `percentiles_i32` format so the two binaries' distribution lines are
/// directly diffable.
fn percentile_summary(sorted: &[i32]) -> String {
    let at = |p: f64| -> i32 {
        let i = ((sorted.len() - 1) as f64 * p).round() as usize;
        sorted[i]
    };
    let mean: f64 = sorted.iter().map(|&x| f64::from(x)).sum::<f64>() / sorted.len() as f64;
    format!(
        "min={} p25={} median={} p75={} max={} mean={mean:.2} n={}",
        sorted[0],
        at(0.25),
        at(0.50),
        at(0.75),
        sorted[sorted.len() - 1],
        sorted.len()
    )
}

/// median/mean of a sample, or an explicit "n/a" for an empty one -- the
/// winner/loser split below can be empty on one side in a small bucket.
fn median_mean(v: &[i32]) -> String {
    if v.is_empty() {
        return "n/a (n=0)".to_string();
    }
    let mut sorted = v.to_vec();
    sorted.sort_unstable();
    let median = sorted[(sorted.len() - 1) / 2];
    let mean: f64 = sorted.iter().map(|&x| f64::from(x)).sum::<f64>() / sorted.len() as f64;
    format!("median={median} mean={mean:.2} n={}", sorted.len())
}

/// Round of the FIRST `Move::WonderStep` a human player-game ever plays,
/// segmented by player count (never pooled -- see this file's own top doc
/// comment on why an n-player game is a structurally different population).
/// This is a SEPARATE observation window from `CostStats`'s round<=3 opening
/// window: it runs over the whole game (the bot's own median first-wonder
/// round is 6, max 14, so round<=3 would see almost nothing) and must not be
/// merged with or substituted for that window, which feeds a published
/// baseline.
#[derive(Default)]
struct WonderRoundStats {
    /// One entry per player-game that built at least one wonder stage.
    rounds: Vec<i32>,
    /// Same rounds, split by this player-game's own outcome; ties/unknown
    /// outcomes land in `rounds` but neither split.
    win_rounds: Vec<i32>,
    loss_rounds: Vec<i32>,
    /// Player-games that never built a wonder stage at all -- its own
    /// labelled bucket, never folded into `rounds`'s tail or dropped.
    never_built: u64,
    /// Every player-game at this count, built or not.
    total: u64,
}

impl WonderRoundStats {
    fn record(&mut self, first_round: Option<i32>, outcome: &str) {
        self.total += 1;
        let Some(r) = first_round else {
            self.never_built += 1;
            return;
        };
        self.rounds.push(r);
        match outcome {
            "win" => self.win_rounds.push(r),
            "loss" => self.loss_rounds.push(r),
            _ => {}
        }
    }

    /// Player-games with no wonder-stage progress by round 4: never-built
    /// ones plus built-but-late ones, matching `behavcensus.rs`'s own
    /// `n_player_games_no_wonder_by_round4` definition (`r > 4`).
    fn no_progress_by_round4(&self) -> u64 {
        self.never_built + self.rounds.iter().filter(|&&r| r > 4).count() as u64
    }

    fn report(&self, players: u8) {
        eprintln!("\nfirst wonder-stage round, {players}p (n={} player-games):", self.total);
        if self.rounds.is_empty() {
            eprintln!("  no player-game at this count ever built a wonder stage");
        } else {
            let mut sorted = self.rounds.clone();
            sorted.sort_unstable();
            eprintln!("  {}", percentile_summary(&sorted));
        }
        let never_pct = 100.0 * self.never_built as f64 / self.total.max(1) as f64;
        eprintln!("  never built a wonder stage at all: {}/{} ({never_pct:.1}%)", self.never_built, self.total);
        let np4 = self.no_progress_by_round4();
        let np4_pct = 100.0 * np4 as f64 / self.total.max(1) as f64;
        eprintln!("  no wonder-stage progress by round 4: {np4}/{} ({np4_pct:.1}%)", self.total);
        eprintln!("  winners: {}", median_mean(&self.win_rounds));
        eprintln!("  losers:  {}", median_mean(&self.loss_rounds));
    }
}

// ---------------------------------------------------------------------
// Tech acquisition -- the human-corpus twin of `bin/behavcensus.rs`'s own
// "Tech acquisition" section (search that file for `TechKind`). Types and
// the `tech_kind` mapper copied verbatim so the two definitions cannot
// drift apart; see that file's doc comment above `TechKind` for the full
// pipeline rationale (SEEN -> TAKEN -> BUILT -> STAFFED) and why `Move::
// Upgrade` is never a BUILT event.

/// The coarse card-type buckets this section reports on. Copied verbatim
/// from `bin/behavcensus.rs`'s own `TechKind`.
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
/// `wildcard_enum_match_arm` is denied repo-wide. Copied verbatim from
/// `bin/behavcensus.rs`'s own `tech_kind`.
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

/// One [`TechKind`]'s tally across the four pipeline stages. Copied
/// verbatim from `bin/behavcensus.rs`'s own `StageCounts`.
#[derive(Default, Clone, Copy)]
struct StageCounts {
    seen: u64,
    taken: u64,
    built: u64,
    staffed: u64,
}

/// Farm/Mine age-tier breakdown for TAKEN and BUILT only. Copied verbatim
/// from `bin/behavcensus.rs`'s own `TierCounts`.
#[derive(Default, Clone, Copy)]
struct TierCounts {
    taken: [u64; 4],
    built: [u64; 4],
}

fn build_kind(kind: CardType) -> &'static str {
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

// ---------------------------------------------------------------------
// Civil card fate -- the human-corpus twin of `bin/behavcensus.rs`'s own
// "Card fate" section (search that file for `taken_rounds`). Tracks every
// civil card a `Move::Take` ever puts into a player's `hand_civil`, by CARD
// IDENTITY, to its eventual resolution: played, culled by age-transition
// antiquation (RULES_SPEC.md \u{a7}12.2), or still in hand at game end --
// mirroring behavcensus's definitions exactly so the two censuses' numbers
// are directly comparable, never a fourth bucket or a different rule.
//
// `Move::Take`'s card_row can only ever hold a civil-track card: reading
// `apply::take_card_impl`, a non-Wonder taken card always goes to
// `hand_civil` unconditionally, and `hand_military` is only ever populated
// by the automatic per-round military draw (`economy.rs`/`events.rs`), never
// by a take -- so `card.get().kind != CardType::Wonder` is already the
// correct, complete "civil card" filter, same as behavcensus's own.
//
// UNLIKE behavcensus's live self-play loop (which drives its own
// `game::step` calls and so has real pre/post `GameState` at every move for
// free), this file only has `replay_game`'s recorded `Decision` list: one
// PRE-move `GameState` plus the `human_move` about to be applied, no
// post-move state stored on the `Decision` itself. So for every decision
// this section clones the pre-move state and calls the SAME public
// `tta::game::step` behavcensus itself uses, to recover a genuine post-move
// state (needed only for the antiquation hand-diff and for the final
// player-game's own true ending round) -- not a second copy of the engine,
// the one existing step function, applied to a move the replayer already
// proved legal.
//
// Gated on `GameResult::completed`: "still in hand at game end" is not a
// meaningful observation for a replay that stopped mid-game (an abandoned
// BGO game, or a replay divergence) -- skipped player-games are counted in
// `CardFateStats::n_incomplete_skipped`, never silently dropped.

/// The three fates a distinct taken civil card (one `taken_rounds` queue
/// entry) resolves to by game end -- mirrors `bin/behavcensus.rs`'s own
/// `CardFate` exactly.
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

/// Tallies of [`CardFate`] across many player-games -- named counters, not
/// an array indexed by an enum discriminant, so a fate nobody hit prints as
/// an explicit `0` rather than a missing key.
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

    fn report(&self, label: &str) {
        if self.taken == 0 {
            eprintln!("  {label}: n=0");
            return;
        }
        let pct = |n: u64| 100.0 * n as f64 / self.taken as f64;
        eprintln!(
            "  {label} ({} taken): played {} ({:.1}%), antiquated {} ({:.1}%), still in hand {} ({:.1}%)",
            self.taken,
            self.played,
            pct(self.played),
            self.antiquated,
            pct(self.antiquated),
            self.still_in_hand,
            pct(self.still_in_hand),
        );
    }
}

/// The `CardId` a civil card was played AS, if `mv` is one of the four
/// sites that ever call `hand_civil.remove_first` in production code
/// (`apply::h_play_leader`, the `Develop` handler, `apply::h_revolution`,
/// `apply::h_play_action`) -- copied verbatim from `bin/behavcensus.rs`'s
/// own `played_civil_card` so the two definitions cannot drift apart.
/// `Move::Build`/`Move::Upgrade` do NOT touch `hand_civil` at all: both
/// operate on a card already sitting in `PlayerState::techs`, so a
/// built/upgraded card's hand-departure already happened, earlier, at
/// whichever `Develop` move put it in the tableau. Exhaustive over every
/// `Move` variant so a future new variant that also drains `hand_civil`
/// cannot silently fall through unclassified.
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
/// than one copy of the same `CardId`). Copied verbatim from
/// `bin/behavcensus.rs`'s own `hand_multiset_diff`.
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

/// Per-seat, per-game scratch state for the card-fate walk -- one FIFO
/// queue of "round taken" per distinct `CardId`, exactly like
/// `bin/behavcensus.rs`'s own `PlayerTrack::taken_rounds`: a queue, not a
/// single value, because a hand can hold more than one physical copy of the
/// same-named card, and `CardList::remove_first` removes the EARLIEST
/// matching instance -- exactly the semantics `pop_front` gives.
struct CardFateTrack {
    taken_rounds: HashMap<CardId, VecDeque<u16>>,
    n_taken: u32,
    n_played: u32,
    n_antiquated: u32,
    /// A Played or Antiquated event that could not be matched back to a
    /// `taken_rounds` entry -- see `CardFateStats::n_mismatch`'s doc
    /// comment; never panics, just counted.
    n_mismatch: u32,
    played_dwell: Vec<i32>,
    /// Antiquation-censored dwell only; the still-in-hand-at-game-end half
    /// of the censored population is computed once, at game end, from
    /// whatever is left in `taken_rounds`.
    censored_dwell: Vec<i32>,
}

impl CardFateTrack {
    fn new() -> Self {
        CardFateTrack {
            taken_rounds: HashMap::new(),
            n_taken: 0,
            n_played: 0,
            n_antiquated: 0,
            n_mismatch: 0,
            played_dwell: Vec::new(),
            censored_dwell: Vec::new(),
        }
    }
}

/// One outcome-group's (all-player-games / winners-only / losers-only)
/// accumulated card-fate numbers.
#[derive(Default)]
struct CardFateGroupStats {
    counts: CardFateCounts,
    taken_per_game: Vec<i32>,
    still_in_hand_per_game: Vec<i32>,
    played_dwell: Vec<i32>,
    censored_dwell: Vec<i32>,
}

impl CardFateGroupStats {
    fn record(
        &mut self,
        n_taken: u32,
        n_played: u32,
        n_antiquated: u32,
        still_in_hand: u64,
        played_dwell: &[i32],
        censored_dwell: &[i32],
    ) {
        self.counts.record(CardFate::Played, u64::from(n_played));
        self.counts.record(CardFate::Antiquated, u64::from(n_antiquated));
        self.counts.record(CardFate::StillInHand, still_in_hand);
        self.taken_per_game.push(n_taken as i32);
        self.still_in_hand_per_game.push(still_in_hand as i32);
        self.played_dwell.extend_from_slice(played_dwell);
        self.censored_dwell.extend_from_slice(censored_dwell);
    }
}

/// Civil-card-fate tally for one player count, split all/winners/losers --
/// never pooled across player counts, same convention as `CostStats`/
/// `WonderRoundStats` above.
#[derive(Default)]
struct CardFateStats {
    n_player_games: u64,
    /// Player-games skipped because `GameResult::completed` was false for
    /// that game's replay -- "still in hand at game end" is not a
    /// meaningful observation for a replay that stopped mid-game.
    n_incomplete_skipped: u64,
    /// A card played or antiquated with no matching `taken_rounds` entry --
    /// see `bin/behavcensus.rs`'s own `n_card_fate_mismatches` doc comment:
    /// that census found 3 of 6177 cards this way on the bot side ("some
    /// path puts a civil card into hand without a Take"); printed as its
    /// own WARNING, never folded silently into the totals above.
    n_mismatch: u64,
    overall: CardFateGroupStats,
    winners: CardFateGroupStats,
    losers: CardFateGroupStats,
}

impl CardFateStats {
    fn report(&self, players: u8) {
        eprintln!(
            "\ncivil card fate, {players}p (n={} player-games, {} skipped as incomplete replays):",
            self.n_player_games, self.n_incomplete_skipped
        );
        if self.n_mismatch > 0 {
            eprintln!(
                "  WARNING  {} card(s) played or antiquated with no matching take -- see \
                 bin/behavcensus.rs's own n_card_fate_mismatches doc comment for the known-shape \
                 counterpart on the bot side",
                self.n_mismatch
            );
        }
        eprintln!("  civil cards taken per player-game: {}", median_mean(&self.overall.taken_per_game));
        eprintln!("    winners: {}", median_mean(&self.winners.taken_per_game));
        eprintln!("    losers:  {}", median_mean(&self.losers.taken_per_game));
        self.overall.counts.report("fate of every taken card, all player-games");
        self.winners.counts.report("fate of every taken card, winners");
        self.losers.counts.report("fate of every taken card, losers");
        eprintln!(
            "  cards still in hand at game end per player-game: {}",
            median_mean(&self.overall.still_in_hand_per_game)
        );
        eprintln!("    winners: {}", median_mean(&self.winners.still_in_hand_per_game));
        eprintln!("    losers:  {}", median_mean(&self.losers.still_in_hand_per_game));
        eprintln!(
            "  dwell in rounds, taken -> played (n={}): {}",
            self.overall.played_dwell.len(),
            median_mean(&self.overall.played_dwell)
        );
        eprintln!(
            "  dwell in rounds, taken -> never played, censored at antiquation/game-end (n={}): {}",
            self.overall.censored_dwell.len(),
            median_mean(&self.overall.censored_dwell)
        );
    }
}

/// This game's per-colour outcome, from the journal's own "WINNER IS" line
/// (`corpus::parse_winner_line`): `("win"|"loss"|"tie", Color)` pairs for
/// every colour a rank clause was found for. Rank 1 alone is "win" (base
/// game 2p has no ties at rank 1 in the corpus scanned for
/// `docs/HUMAN_WINNERS.md`-adjacent work); every other parsed rank is
/// "loss". Empty if no "WINNER IS" clause parsed at all.
fn parse_outcomes(text: &str) -> Vec<(Color, &'static str)> {
    let Some(pos) = text.find("WINNER IS") else { return Vec::new() };
    parse_winner_line(&text[pos..])
        .into_iter()
        .map(|(rank, color, _score)| (color, if rank == 1 { "win" } else { "loss" }))
        .collect()
}

fn run(index_path: &str, journals_dir: &str) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;

    // Cost distributions are segmented by player count (`meta.players`) and
    // reported separately per count -- never pooled -- because take-cost
    // economics differ by card-row width, which is itself a function of
    // player count; a pooled number would silently mix populations.
    let mut cost_stats: BTreeMap<u8, CostStats> = BTreeMap::new();

    // First-wonder-stage-round distribution, segmented by player count --
    // never pooled (see `WonderRoundStats`'s own doc comment).
    let mut wonder_stats: BTreeMap<u8, WonderRoundStats> = BTreeMap::new();

    // Civil-card-fate distribution, segmented by player count -- never
    // pooled (see the "Civil card fate" section's own doc comment above).
    let mut card_fate_stats: BTreeMap<u8, CardFateStats> = BTreeMap::new();

    // Production curve (3-player games only, see the accumulation site
    // below): (sum food, sum resources, n) per round, the human twin of
    // `bin/behavcensus.rs`'s `Report::production_by_round`. A `BTreeMap` so
    // printing needs no separate sort, unlike behavcensus's `HashMap`.
    let mut production_by_round: BTreeMap<u16, (u64, u64, u64)> = BTreeMap::new();

    // Worker allocation curve (3-player games only, same accumulation site
    // as `production_by_round` above): the human twin of
    // `bin/behavcensus.rs`'s `Report::alloc_by_round`.
    let mut alloc_by_round: BTreeMap<u16, AllocAccum> = BTreeMap::new();

    // Tech acquisition (3-player games only, same accumulation site as
    // `production_by_round`/`alloc_by_round` above): the human twin of
    // `bin/behavcensus.rs`'s `Report::tech_acq`/`farm_tier`/`mine_tier`.
    // `tech_acq_player_games` is the denominator for every per-player-game
    // rate printed below -- every player-game in a 3p replay contributes
    // exactly one sample, so no separate counter is needed per `TechKind`.
    let mut tech_acq: HashMap<TechKind, StageCounts> = HashMap::new();
    let mut farm_tier = TierCounts::default();
    let mut mine_tier = TierCounts::default();
    let mut tech_acq_player_games: u64 = 0;

    for meta in games.iter() {
        let n = meta.players as usize;
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let outcomes_by_color = parse_outcomes(&text);
        let result = replay_game(meta, &text, &card_index, true);

        // Per-seat opening trackers, round <= 3 only, one slot per seat.
        let mut first_take: Vec<Option<(&'static str, i32)>> = vec![None; n];
        let mut first_build: Vec<Option<(&'static str, &'static str)>> = vec![None; n];
        let mut took_leader: Vec<bool> = vec![false; n];
        let mut increased_pop: Vec<bool> = vec![false; n];
        let mut ca_unused: Vec<i32> = vec![0; n];

        for d in &result.decisions {
            if d.state.round > 3 {
                continue;
            }
            let seat = d.state.current as usize;
            if seat >= n {
                continue;
            }
            match d.human_move {
                Move::Take { slot } => {
                    if first_take[seat].is_none() {
                        let card = d.state.card_row[slot as usize];
                        if !card.is_none() {
                            let cost = tta::costs::take_cost(&d.state, &d.state.players[seat], slot as usize);
                            first_take[seat] = Some((card.get().name, cost));
                        }
                    }
                }
                Move::Build { card } | Move::Develop { card, .. } => {
                    if first_build[seat].is_none() {
                        first_build[seat] = Some((build_kind(card.get().kind), card.get().name));
                    }
                }
                Move::Upgrade { to, .. } => {
                    if first_build[seat].is_none() {
                        first_build[seat] = Some((build_kind(to.get().kind), to.get().name));
                    }
                }
                Move::PlayLeader { .. } => {
                    took_leader[seat] = true;
                }
                Move::Pop { .. } | Move::PopFree => {
                    increased_pop[seat] = true;
                }
                Move::EndTurn => {
                    let ca = d.state.players[seat].civil_actions;
                    if ca > 0 {
                        ca_unused[seat] += ca as i32;
                    }
                }
                Move::WonderStep { .. } | Move::Revolution { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::PolPass | Move::Resign => {}
            }
        }

        // First-wonder-stage-round tracker: a SEPARATE pass over the whole
        // game (no round cap), because the loop above intentionally stops at
        // round 3 and that window must not move. `Move::WonderStep` is the
        // only variant of interest here, matched explicitly against the
        // full variant list so a future new `Move` case cannot fall through
        // silently.
        let mut first_wonder_round: Vec<Option<i32>> = vec![None; n];
        for d in &result.decisions {
            let seat = d.state.current as usize;
            if seat >= n {
                continue;
            }
            match d.human_move {
                Move::WonderStep { .. } => {
                    if first_wonder_round[seat].is_none() {
                        first_wonder_round[seat] = Some(d.state.round as i32);
                    }
                }
                Move::Take { .. } | Move::Build { .. } | Move::Develop { .. } | Move::Upgrade { .. } | Move::Pop { .. } | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => {}
            }
        }

        for seat in 0..n {
            let (take_name, take_cost) = first_take[seat].unwrap_or(("none", -1));
            let (build_kind_s, build_name) = first_build[seat].unwrap_or(("none", "none"));
            let outcome = Color::from_seat(seat as u8)
                .and_then(|c| outcomes_by_color.iter().find(|(oc, _)| *oc == c))
                .map(|(_, o)| *o)
                .unwrap_or("unknown");
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                meta.id,
                seat,
                take_name,
                take_cost,
                build_kind_s,
                build_name,
                took_leader[seat],
                increased_pop[seat],
                ca_unused[seat],
                outcome,
            );
            if first_take[seat].is_some() {
                cost_stats.entry(meta.players).or_default().record(take_cost);
            }
            wonder_stats.entry(meta.players).or_default().record(first_wonder_round[seat], outcome);
        }

        // ---- production curve (3-player games only): mean worker-capped
        // food/resource production (`economy::production_this_turn`) per
        // round, one sample per player-round at the START of that player's
        // turn -- same call and the same print format as
        // `bin/behavcensus.rs`'s own "Production curve" section, so the two
        // curves are directly comparable. `result.decisions` already
        // carries the PRE-move `GameState` behavcensus's live self-play
        // loop gets for free (see this file's own top doc comment);
        // `d.state.current` only ever changes at `end_turn`'s
        // `state.current = nxt` (game.rs), so "actor differs from the last
        // one sampled" is exactly "this is the first move of a new turn",
        // the same detection `bin/behavcensus.rs::play_one` uses.
        if meta.players == 3 {
            let mut prev_actor: Option<u8> = None;
            // Per-seat running sets for the tech-acquisition pipeline (see
            // `bin/behavcensus.rs`'s "Tech acquisition" section doc comment
            // above `TechKind`) -- folded into the file-level `tech_acq`/
            // `farm_tier`/`mine_tier` accumulators once this game's decision
            // loop below finishes.
            let mut seat_seen: Vec<HashSet<CardId>> = (0..n).map(|_| HashSet::new()).collect();
            let mut seat_taken: Vec<HashSet<CardId>> = (0..n).map(|_| HashSet::new()).collect();
            let mut seat_built: Vec<HashSet<CardId>> = (0..n).map(|_| HashSet::new()).collect();
            let mut seat_staffed: Vec<HashSet<CardId>> = (0..n).map(|_| HashSet::new()).collect();
            for d in &result.decisions {
                let seat = d.state.current;
                if seat as usize >= n {
                    continue;
                }
                // ---- tech acquisition: TAKEN -- every decision point, not
                // just turn-start (a Take can happen anywhere in a turn).
                // See `bin/behavcensus.rs`'s identical block for the full
                // rationale.
                if let Move::Take { slot } = d.human_move {
                    let card = d.state.card_row[slot as usize];
                    if !card.is_none() && tech_kind(card.get().kind).is_some() {
                        seat_taken[seat as usize].insert(card);
                    }
                }
                if prev_actor != Some(seat) {
                    prev_actor = Some(seat);
                    let (food, resources) = tta::economy::production_this_turn(&d.state, seat);
                    let e = production_by_round.entry(d.state.round).or_insert((0u64, 0u64, 0u64));
                    e.0 += u64::from(food);
                    e.1 += u64::from(resources);
                    e.2 += 1;

                    // ---- worker allocation: SAME instant, same player,
                    // classifying every placed worker by `CardType` and
                    // reading the printed per-worker production of every
                    // Farm/Mine tech held (whether staffed or not). See
                    // `bin/behavcensus.rs`'s identical block for the full
                    // rationale; exhaustive match, no wildcard arm.
                    let p = &d.state.players[seat as usize];
                    let mut farm_workers = 0u32;
                    let mut mine_workers = 0u32;
                    let mut urban_workers = 0u32;
                    let mut mil_workers = 0u32;
                    let mut best_farm = 0i16;
                    let mut best_mine = 0i16;
                    for (id, slot) in p.techs.iter() {
                        let workers = u32::from(slot.workers);
                        // ---- tech acquisition: BUILT/STAFFED -- same
                        // turn-start instant, same `p.techs` walk as the
                        // alloc classification just below. See
                        // `bin/behavcensus.rs`'s identical block for the
                        // full rationale.
                        if tech_kind(id.get().kind).is_some() {
                            seat_built[seat as usize].insert(id);
                            if workers > 0 {
                                seat_staffed[seat as usize].insert(id);
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
                    let a = alloc_by_round.entry(d.state.round).or_default();
                    a.farm_workers += u64::from(farm_workers);
                    a.mine_workers += u64::from(mine_workers);
                    a.urban_workers += u64::from(urban_workers);
                    a.mil_workers += u64::from(mil_workers);
                    a.free_workers += u64::from(p.workers_free);
                    a.staffed_workers += u64::from(farm_workers + mine_workers + urban_workers + mil_workers);
                    a.best_farm_sum += u64::from(best_farm.max(0) as u16);
                    a.best_mine_sum += u64::from(best_mine.max(0) as u16);
                    a.n += 1;

                    // ---- tech acquisition: SEEN -- same turn-start
                    // instant, every distinct tracked-type card visible in
                    // the card row right now. See `bin/behavcensus.rs`'s
                    // identical block for the full rationale.
                    for &card in &d.state.card_row {
                        if !card.is_none() && tech_kind(card.get().kind).is_some() {
                            seat_seen[seat as usize].insert(card);
                        }
                    }
                }
            }

            // ---- tech acquisition: fold this game's per-seat running sets
            // into the file-level accumulators -- see `bin/behavcensus.rs`'s
            // end-of-game aggregation for the identical fold, just done once
            // per game here instead of once per player-game inside a shared
            // `Report`.
            for seat in 0..n {
                for &card in &seat_seen[seat] {
                    if let Some(kind) = tech_kind(card.get().kind) {
                        tech_acq.entry(kind).or_default().seen += 1;
                    }
                }
                for &card in &seat_taken[seat] {
                    if let Some(kind) = tech_kind(card.get().kind) {
                        tech_acq.entry(kind).or_default().taken += 1;
                        let tier = card.level() as usize;
                        match kind {
                            TechKind::Farm => {
                                if let Some(s) = farm_tier.taken.get_mut(tier) {
                                    *s += 1;
                                }
                            }
                            TechKind::Mine => {
                                if let Some(s) = mine_tier.taken.get_mut(tier) {
                                    *s += 1;
                                }
                            }
                            TechKind::Lab | TechKind::Temple | TechKind::Arena | TechKind::Library | TechKind::Military => {}
                        }
                    }
                }
                for &card in &seat_built[seat] {
                    if let Some(kind) = tech_kind(card.get().kind) {
                        tech_acq.entry(kind).or_default().built += 1;
                        let tier = card.level() as usize;
                        match kind {
                            TechKind::Farm => {
                                if let Some(s) = farm_tier.built.get_mut(tier) {
                                    *s += 1;
                                }
                            }
                            TechKind::Mine => {
                                if let Some(s) = mine_tier.built.get_mut(tier) {
                                    *s += 1;
                                }
                            }
                            TechKind::Lab | TechKind::Temple | TechKind::Arena | TechKind::Library | TechKind::Military => {}
                        }
                    }
                }
                for &card in &seat_staffed[seat] {
                    if let Some(kind) = tech_kind(card.get().kind) {
                        tech_acq.entry(kind).or_default().staffed += 1;
                    }
                }
                tech_acq_player_games += 1;
            }
        }

        // ---- civil card fate: see the "Civil card fate" section's own doc
        // comment above for why this needs its own recomputed post-move
        // state per decision (unlike the round<=3 opening trackers above,
        // which only ever read pre-move state).
        if result.completed && !result.decisions.is_empty() {
            let mut fate_tracks: Vec<CardFateTrack> = (0..n).map(|_| CardFateTrack::new()).collect();
            let mut final_round: u16 = result.decisions[0].state.round;

            for d in &result.decisions {
                let actor = d.state.current as usize;
                if actor >= n {
                    continue;
                }
                let round_before = d.state.round;
                let taken_card = match d.human_move {
                    Move::Take { slot } => Some(d.state.card_row[slot as usize]),
                    Move::Build { .. } | Move::Develop { .. } | Move::Upgrade { .. } | Move::WonderStep { .. } | Move::Pop { .. } | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => None,
                };
                let played_this_move = played_civil_card(d.human_move);
                let pre_age = d.state.age_civil;
                let pre_hands: Vec<Vec<CardId>> =
                    (0..n).map(|i| d.state.players[i].hand_civil.as_slice().to_vec()).collect();

                // The genuine post-move `GameState`, stored on the
                // `Decision` by `replay_common::Replayer::apply_move` (see
                // `Decision::state_after`'s own doc). The old workaround --
                // re-running `game::step` on a clone of the pre-state --
                // was blind to the replayer's own age force (it happens on
                // the NEXT journal line, not inside this decision's move)
                // and is what made this census's antiquation count read 0.
                let post_state = match &d.state_after {
                    Some(s) => s,
                    None => continue,
                };
                final_round = post_state.round;

                if let Some(card) = taken_card {
                    if !card.is_none() && card.get().kind != CardType::Wonder {
                        fate_tracks[actor].taken_rounds.entry(card).or_default().push_back(round_before);
                        fate_tracks[actor].n_taken += 1;
                    }
                }

                if let Some(card) = played_this_move {
                    let t = &mut fate_tracks[actor];
                    match t.taken_rounds.get_mut(&card).and_then(VecDeque::pop_front) {
                        Some(taken_round) => {
                            t.n_played += 1;
                            t.played_dwell.push(round_before as i32 - taken_round as i32);
                        }
                        None => t.n_mismatch += 1,
                    }
                }

                // A card that LEFT the actor's own hand this move for a
                // reason OTHER than the tracked play above is a real
                // untracked hand-population path (or an untracked removal) --
                // counted in `n_mismatch`, not silently swallowed: the old
                // code only diffed hands across an age change, so a
                // same-age removal was invisible to both the played and
                // antiquated counters.
                if let Some(played) = played_this_move {
                    let removed = hand_multiset_diff(&pre_hands[actor], post_state.players[actor].hand_civil.as_slice());
                    let extra: Vec<CardId> = removed
                        .into_iter()
                        .filter(|c| *c != played)
                        .collect();
                    let t = &mut fate_tracks[actor];
                    for card in extra {
                        match t.taken_rounds.get_mut(&card).and_then(VecDeque::pop_front) {
                            Some(taken_round) => {
                                t.n_antiquated += 1;
                                t.censored_dwell.push(round_before as i32 - taken_round as i32);
                            }
                            None => t.n_mismatch += 1,
                        }
                    }
                }

                if post_state.age_civil != pre_age {
                    for i in 0..n {
                        let post_hand = post_state.players[i].hand_civil.as_slice();
                        let mut removed = hand_multiset_diff(&pre_hands[i], post_hand);
                        if i == actor {
                            if let Some(played) = played_this_move {
                                if let Some(pos) = removed.iter().position(|&c| c == played) {
                                    removed.remove(pos);
                                }
                            }
                        }
                        let t = &mut fate_tracks[i];
                        for card in removed {
                            match t.taken_rounds.get_mut(&card).and_then(VecDeque::pop_front) {
                                Some(taken_round) => {
                                    t.n_antiquated += 1;
                                    t.censored_dwell.push(round_before as i32 - taken_round as i32);
                                }
                                None => t.n_mismatch += 1,
                            }
                        }
                    }
                }
            }

            let stats = card_fate_stats.entry(meta.players).or_default();
            for (seat, track) in fate_tracks.iter().enumerate() {
                let mut still_in_hand: u64 = 0;
                let mut censored_dwell_this_game = track.censored_dwell.clone();
                for rounds in track.taken_rounds.values() {
                    for &taken_round in rounds {
                        still_in_hand += 1;
                        censored_dwell_this_game.push(final_round as i32 - taken_round as i32);
                    }
                }
                let n_taken = track.n_taken;
                let n_played = track.n_played;
                let n_antiquated = track.n_antiquated;
                let played_dwell_this_game = &track.played_dwell;
                let outcome = Color::from_seat(seat as u8)
                    .and_then(|c| outcomes_by_color.iter().find(|(oc, _)| *oc == c))
                    .map(|(_, o)| *o)
                    .unwrap_or("unknown");

                stats.n_player_games += 1;
                stats.n_mismatch += u64::from(track.n_mismatch);
                stats.overall.record(
                    n_taken,
                    n_played,
                    n_antiquated,
                    still_in_hand,
                    played_dwell_this_game,
                    &censored_dwell_this_game,
                );
                match outcome {
                    "win" => stats.winners.record(
                        n_taken,
                        n_played,
                        n_antiquated,
                        still_in_hand,
                        played_dwell_this_game,
                        &censored_dwell_this_game,
                    ),
                    "loss" => stats.losers.record(
                        n_taken,
                        n_played,
                        n_antiquated,
                        still_in_hand,
                        played_dwell_this_game,
                        &censored_dwell_this_game,
                    ),
                    _ => {}
                }
            }
        } else {
            // Not `result.completed`, or (vanishingly rare) a completed
            // replay with zero recorded decisions -- either way there is no
            // usable "game end" to measure a still-in-hand fate against, so
            // this player-game is counted as skipped rather than silently
            // dropped.
            card_fate_stats.entry(meta.players).or_default().n_incomplete_skipped += n as u64;
        }
    }

    for (players, stats) in &cost_stats {
        stats.report(*players);
    }
    for (players, stats) in &wonder_stats {
        stats.report(*players);
    }
    for (players, stats) in &card_fate_stats {
        stats.report(*players);
    }

    eprintln!("\n### Production curve\n");
    for (round, (food_sum, resources_sum, n)) in &production_by_round {
        eprintln!(
            "round {round}: food mean={:.2} resources mean={:.2} n={n}",
            *food_sum as f64 / (*n).max(1) as f64,
            *resources_sum as f64 / (*n).max(1) as f64
        );
    }

    eprintln!("\n### Worker allocation curve\n");
    for (round, a) in &alloc_by_round {
        let n = a.n.max(1) as f64;
        eprintln!(
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
    // `TechKind` bucket, one sample per player-game. See
    // `bin/behavcensus.rs`'s identical print block for the full rationale.
    eprintln!("\n### Tech acquisition\n");
    let tacq_n = tech_acq_player_games.max(1) as f64;
    eprintln!("per player-game (n={tech_acq_player_games}):");
    eprintln!("{:<10} {:>8} {:>8} {:>8} {:>8}", "type", "seen", "taken", "built", "staffed");
    for kind in ALL_TECH_KINDS {
        let c = tech_acq.get(&kind).copied().unwrap_or_default();
        eprintln!(
            "{:<10} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
            tech_kind_label(kind),
            c.seen as f64 / tacq_n,
            c.taken as f64 / tacq_n,
            c.built as f64 / tacq_n,
            c.staffed as f64 / tacq_n,
        );
    }
    eprintln!("\nFarm/Mine taken by age tier, per player-game:");
    eprintln!(
        "  Farm: A={:.3} I={:.3} II={:.3} III={:.3}",
        farm_tier.taken[0] as f64 / tacq_n,
        farm_tier.taken[1] as f64 / tacq_n,
        farm_tier.taken[2] as f64 / tacq_n,
        farm_tier.taken[3] as f64 / tacq_n,
    );
    eprintln!(
        "  Mine: A={:.3} I={:.3} II={:.3} III={:.3}",
        mine_tier.taken[0] as f64 / tacq_n,
        mine_tier.taken[1] as f64 / tacq_n,
        mine_tier.taken[2] as f64 / tacq_n,
        mine_tier.taken[3] as f64 / tacq_n,
    );
    eprintln!("Farm/Mine built by age tier, per player-game:");
    eprintln!(
        "  Farm: A={:.3} I={:.3} II={:.3} III={:.3}",
        farm_tier.built[0] as f64 / tacq_n,
        farm_tier.built[1] as f64 / tacq_n,
        farm_tier.built[2] as f64 / tacq_n,
        farm_tier.built[3] as f64 / tacq_n,
    );
    eprintln!(
        "  Mine: A={:.3} I={:.3} II={:.3} III={:.3}",
        mine_tier.built[0] as f64 / tacq_n,
        mine_tier.built[1] as f64 / tacq_n,
        mine_tier.built[2] as f64 / tacq_n,
        mine_tier.built[3] as f64 / tacq_n,
    );
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 2 {
        eprintln!("usage: humanopenings <index.tsv> <journals_dir>");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1]) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real 2p BGO end-of-game clause: rank 1 must map to "win", rank 2 to
    /// "loss" -- this is the whole contract `run`'s outcome lookup relies
    /// on, so a regression here (e.g. an off-by-one on `rank`) would flip
    /// every win-rate number in `OPENINGS.txt` silently.
    #[test]
    fn parse_outcomes_maps_rank_1_to_win_and_rank_2_to_loss() {
        let text = "...\tWINNER IS PLAYER AS ORANGE (195 PTS); 2nd is PLAYER as Purple (160 pts)\n";
        let outcomes = parse_outcomes(text);
        assert_eq!(outcomes, vec![(Color::Orange, "win"), (Color::Purple, "loss")]);
    }

    /// No "WINNER IS" clause anywhere in the text (e.g. a game journal that
    /// stops mid-play with no recorded ending) must yield an empty outcome
    /// list, not a panic -- `run`'s per-seat lookup then correctly falls
    /// back to "unknown" via its own `unwrap_or`.
    #[test]
    fn parse_outcomes_is_empty_when_no_winner_clause_is_present() {
        assert_eq!(parse_outcomes("no ending here\n"), Vec::new());
    }
}
