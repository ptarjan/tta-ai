//! `takeprobe` -- the decisive experiment behind
//! `analysis/mine_take_probe_3p_2026-08-24.txt`, following up
//! `analysis/tech_acquisition_3p_2026-08-24.txt`'s finding that a
//! non-starting Mine card (Iron/Coal/Oil) is SEEN in the card row in
//! essentially every player-game but TAKEN (per `behavcensus.rs`'s pre-move
//! `taken_card` variable) exactly 0.000 times at every age tier, across 600
//! self-play player-games.
//!
//! That census answers WHAT HAPPENED; it explicitly does not answer WHY
//! (`tech_acquisition_3p_2026-08-24.txt` caveat 6). This binary is read-only
//! instrumentation aimed at WHY, and measures independently of
//! `behavcensus.rs`'s `taken_card` mechanism entirely -- it never reads or
//! computes a `taken_card`-shaped value -- so the two are a genuine
//! cross-check, not two views of the same counter.
//!
//! # Method
//!
//! Self-plays games at `--players` seats, all seats sharing one
//! `--weights` champion vector (mirrors `behavcensus.rs::play_one`'s
//! all-seats-`BotKind::Weighted` self-play mirror match -- confirmed by
//! reading `behavcensus.rs:1337-1338`, which is what the census this probe
//! follows up on actually ran). At every decision point where a civil take
//! is structurally possible (`state.phase == Phase::Actions`,
//! `state.pending.is_empty()` -- the only site `legal.rs::action_moves`
//! generates `Move::Take` from), for each of the three non-starting Mine
//! cards (Iron/Coal/Oil) found sitting in `state.card_row` at that instant,
//! records four pipeline stages:
//!
//! 1. **SEEN** -- the card is in the row at a decision point where a take is
//!    possible (by construction, every card counted here satisfies this).
//! 2. **LEGAL** -- `Move::Take{slot}` for that exact row slot is present in
//!    `legal::legal_moves(&state)`, i.e. `costs::can_take_gated` passed.
//! 3. **SCORED** -- it survives into the candidate set
//!    [`tta::bots::weighted::eval::WeightedBot`] actually evaluates.
//!    `WeightedBot::choose`/`rank_moves` both start from
//!    `bots::filter_resign`, which removes only `Move::Resign` -- never a
//!    `Take` -- so for this bot kind SCORED is structurally identical to
//!    LEGAL whenever LEGAL holds: there is no shortlist/narrowing stage
//!    between move generation and evaluation for a plain 1-ply
//!    `WeightedBot`, unlike a beam/shortlist search. The probe still checks
//!    this independently (searching the candidate list
//!    [`tta::bots::weighted::eval::candidate_features`] actually returns for
//!    the move) rather than assuming it, so a future change to
//!    `filter_resign` or a narrowing stage would be caught here, not assumed
//!    away.
//! 4. **if SCORED**: the move's own value, the winning candidate's value,
//!    and the move's 1-based rank among every scored candidate, computed by
//!    dotting [`tta::bots::weighted::eval::candidate_features`]'s per-move
//!    feature vectors against the champion weights via
//!    [`tta::bots::weighted::eval::dot`] (the strict-`>`, first-wins argmax
//!    over those dotted scores reproduces `WeightedBot::choose`'s own pick
//!    exactly -- see [`play_one`]'s inline comment -- so driving the game
//!    off it reproduces an ordinary `BotKind::Weighted` self-play trajectory
//!    exactly; this probe never diverges from what an unwatched self-play
//!    game would have played).
//!
//! # Stage-4 diagnosis: what beats a scored Mine take, and why
//!
//! At every decision point where a Mine take is SCORED, this binary also
//! records (a) the [`Move`] kind that won instead ([`move_kind`]) and (b)
//! how much each [`WeightKey`] term contributes to the score gap between
//! the winner and the Mine take -- `weight(k) * (winner_feature(k) -
//! take_feature(k))`, via [`candidate_features`]/[`dot`] (the same vectors
//! `bin/agreefit.rs` fits against, guaranteed by that module's own doc
//! comment and tests to dot back to `evaluate`'s exact score). Reported as
//! a RANKED list of key NAMES with the SIGN of their net effect only --
//! never a raw contribution number or a weight value -- per this task's own
//! hard constraint against writing down a weight value or coefficient; this
//! binary reports where the gap comes from, not what any term should be.
//!
//! ```text
//! cargo run --release --bin takeprobe -- \
//!     --games 200 --players 3 --weights /path/to/champ3p.json --threads 5
//! ```

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tta::apply;
use tta::bots::weighted::eval::{candidate_features, dot, load_weights, WeightedBot};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::game::{self, MOVE_CAP};
use tta::legal;
use tta::{CardId, Move, Phase};

/// The three non-starting mine cards, in age order. Bronze (age A) is the
/// starting technology printed on the player board, never a deck card
/// (`data/cards_civil.json`'s own note on it) -- it is never in `card_row`
/// and is deliberately excluded here, matching
/// `tech_acquisition_3p_2026-08-24.txt`'s own SEEN/TAKEN definition.
const MINE_NAMES: [&str; 3] = ["Iron", "Coal", "Oil"];

#[derive(Clone, Copy, Default)]
struct CardStats {
    /// Stage 1: decision points where this card sat in `card_row` and a
    /// take was structurally possible.
    seen: u64,
    /// Stage 2: of those, `Move::Take{slot}` for this card was legal.
    legal: u64,
    /// Stage 3: of those legal takes, the move survived into the candidate
    /// set `rank_moves` actually scored.
    scored: u64,
    /// Stage 4, summed over every scored occurrence: this move's rank
    /// (1 == it won) and the score gap to the winner.
    rank_sum: u64,
    gap_sum: f64,
    /// Stage 4: the best (lowest) rank a take of this card ever achieved.
    best_rank: Option<u64>,
}

impl CardStats {
    fn record_scored(&mut self, rank: u64, gap: f64) {
        self.scored += 1;
        self.rank_sum += rank;
        self.gap_sum += gap;
        self.best_rank = Some(self.best_rank.map_or(rank, |b| b.min(rank)));
    }

    fn merge(&mut self, other: &CardStats) {
        self.seen += other.seen;
        self.legal += other.legal;
        self.scored += other.scored;
        self.rank_sum += other.rank_sum;
        self.gap_sum += other.gap_sum;
        self.best_rank = match (self.best_rank, other.best_rank) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, b) => b,
        };
    }
}

/// The variant name of a [`Move`], read off its `Debug` output rather than a
/// hand-written `match` -- `Move` is a large enum and `Cargo.toml` denies
/// `wildcard_enum_match_arm`, so an exhaustive `match` here just to recover
/// a label would be ~30 arms of pure bookkeeping this diagnostic does not
/// need to own. `{mv:?}` renders e.g. `"Take { slot: 3 }"` or `"EndTurn"`;
/// the first token before a space or `(` is the variant name in both the
/// struct-variant and tuple-variant shapes `Move` uses.
fn move_kind(mv: &Move) -> String {
    let s = format!("{mv:?}");
    s.split(['(', ' ']).next().unwrap_or(&s).to_string()
}

/// Stage-4 diagnosis shared across every decision point where a Mine take
/// was SCORED: what beat it (by [`Move`] kind), and which [`WeightKey`]
/// terms account for the score gap. `key_contrib_sum[k]` accumulates
/// `weight(k) * (winner_feature(k) - take_feature(k))` -- positive means
/// that key widens the winner's advantage over the Mine take that
/// occurrence, negative means it narrows it -- summed over every scored
/// occurrence of every Mine card, ready to average and rank by `|mean|` at
/// report time. Deliberately never printed as a raw number in the report
/// (see `bin/takeprobe.rs`'s own top doc comment / the task's hard
/// constraint against writing down a weight value or coefficient) -- only
/// the ranked key NAMES and their sign are reported.
struct Diag {
    winner_kinds: HashMap<String, u64>,
    key_contrib_sum: Vec<f64>,
    key_contrib_n: u64,
}

impl Diag {
    fn new() -> Diag {
        Diag { winner_kinds: HashMap::new(), key_contrib_sum: vec![0.0; WeightKey::ALL.len()], key_contrib_n: 0 }
    }

    fn merge(&mut self, other: &Diag) {
        for (k, &v) in &other.winner_kinds {
            *self.winner_kinds.entry(k.clone()).or_insert(0) += v;
        }
        for i in 0..self.key_contrib_sum.len() {
            self.key_contrib_sum[i] += other.key_contrib_sum[i];
        }
        self.key_contrib_n += other.key_contrib_n;
    }
}

/// Resolves [`MINE_NAMES`] to [`CardId`]s once. Returns an error string
/// (rather than panicking) if the static card table ever loses one of these
/// names, so a bad `--weights`/table mismatch reports cleanly instead of
/// aborting with a bare `unwrap` panic.
fn mine_card_ids() -> Result<[CardId; 3], String> {
    let mut ids = [CardId::NONE; 3];
    for (i, &name) in MINE_NAMES.iter().enumerate() {
        ids[i] = CardId::by_name(name).ok_or_else(|| format!("card table has no {name:?}"))?;
    }
    Ok(ids)
}

/// Plays one self-play game to completion (or to [`MOVE_CAP`]), recording
/// [`CardStats`] for each of [`MINE_NAMES`]. Never mutates anything outside
/// its own locals -- a fresh `GameState` from `game::new_game` and a fresh
/// `WeightedBot` -- and never calls `apply::apply` on anything but the
/// move the bot itself already chose, so this reproduces an ordinary
/// self-play trajectory exactly; it only ever ADDS observation, never
/// changes what gets played.
fn play_one(players: u8, weights: Weights, seed: u64, mine_ids: &[CardId; 3]) -> ([CardStats; 3], Diag) {
    let bot = WeightedBot::new(weights);
    let mut state = game::new_game(players, seed);
    let mut stats = [CardStats::default(); 3];
    let mut diag = Diag::new();
    let mut moves_played = 0usize;

    while !state.game_over {
        if moves_played >= MOVE_CAP {
            break;
        }
        moves_played += 1;

        let moves = legal::legal_moves(&state);
        if moves.as_slice().is_empty() {
            break;
        }

        let take_possible = state.phase == Phase::Actions && state.pending.is_empty();
        let mut present: Vec<(usize, u8)> = Vec::new(); // (mine_ids index, row slot)
        if take_possible {
            for (slot, &id) in state.card_row.iter().enumerate() {
                if let Some(mi) = mine_ids.iter().position(|&m| m == id) {
                    present.push((mi, slot as u8));
                }
            }
        }

        let mv = if present.is_empty() {
            bot.choose(&state, moves.as_slice())
        } else {
            // `candidate_features` shares the exact same root/ctx/trial
            // construction `WeightedBot::choose`/`rank_moves` use (its own
            // doc comment), and `dot(w, f)` reproduces `evaluate`'s score
            // exactly, `EndTurnBias` included -- so picking the strict-`>`
            // argmax over `dot(w, f)` here, first-candidate-wins on ties,
            // reproduces `choose`'s own pick exactly (same tie-break rule),
            // while additionally exposing every candidate's per-`WeightKey`
            // feature vector for the stage-4 key-contribution breakdown.
            let feats = candidate_features(&state, moves.as_slice(), false, &weights);
            let vals: Vec<f64> = feats.iter().map(|(_, f)| dot(&weights, f)).collect();
            let mut best_idx = 0usize;
            for (i, &v) in vals.iter().enumerate().skip(1) {
                if v > vals[best_idx] {
                    best_idx = i;
                }
            }
            let winner_val = vals[best_idx];
            for (mi, slot) in present {
                stats[mi].seen += 1;
                let take_mv = Move::Take { slot };
                if moves.as_slice().contains(&take_mv) {
                    stats[mi].legal += 1;
                    if let Some(pos) = feats.iter().position(|(m, _)| *m == take_mv) {
                        let take_val = vals[pos];
                        let rank = vals.iter().filter(|&&v| v > take_val).count() as u64 + 1;
                        stats[mi].record_scored(rank, winner_val - take_val);
                        for (ki, contrib) in diag.key_contrib_sum.iter_mut().enumerate() {
                            let w = weights.get(WeightKey::ALL[ki]);
                            *contrib += w * (feats[best_idx].1[ki] - feats[pos].1[ki]);
                        }
                        diag.key_contrib_n += 1;
                        *diag.winner_kinds.entry(move_kind(&feats[best_idx].0)).or_insert(0) += 1;
                    }
                }
            }
            feats[best_idx].0
        };
        apply::apply(&mut state, mv);
    }
    (stats, diag)
}

struct Args {
    games: usize,
    players: u8,
    weights_path: String,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args { games: 200, players: 3, weights_path: String::new(), seed: 1, threads: 1 }
    }
}

const USAGE: &str = "\
usage: takeprobe --weights PATH [options]

  --games N       games to play (default 200)
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

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("takeprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let weights = match load_weights(std::path::Path::new(&args.weights_path)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("takeprobe: loading {}: {e}", args.weights_path);
            return ExitCode::FAILURE;
        }
    };

    let mine_ids = match mine_card_ids() {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("takeprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games).max(1);
    let totals: Mutex<[CardStats; 3]> = Mutex::new([CardStats::default(); 3]);
    let diag_totals: Mutex<Diag> = Mutex::new(Diag::new());

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(|| {
                let mut local = [CardStats::default(); 3];
                let mut local_diag = Diag::new();
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= args.games {
                        break;
                    }
                    let seed = args.seed + g as u64;
                    let (s, d) = play_one(args.players, weights, seed, &mine_ids);
                    for i in 0..3 {
                        local[i].merge(&s[i]);
                    }
                    local_diag.merge(&d);
                }
                let mut t = totals.lock().expect("totals mutex poisoned");
                for i in 0..3 {
                    t[i].merge(&local[i]);
                }
                diag_totals.lock().expect("diag mutex poisoned").merge(&local_diag);
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    });

    let totals = totals.into_inner().expect("totals mutex poisoned");
    let diag = diag_totals.into_inner().expect("diag mutex poisoned");
    let elapsed = start.elapsed();

    println!("takeprobe: {} games, {} players, weights={}", args.games, args.players, args.weights_path);
    println!("elapsed: {elapsed:.1?}");
    println!();
    println!(
        "{:<6} {:>8} {:>8} {:>8}   {:>6} {:>6}   {:>10} {:>9}",
        "card", "seen", "legal", "scored", "L-rate", "S-rate", "mean_gap", "best_rank"
    );
    for (i, &name) in MINE_NAMES.iter().enumerate() {
        let s = &totals[i];
        let legal_rate = if s.seen > 0 { s.legal as f64 / s.seen as f64 } else { 0.0 };
        let scored_rate = if s.legal > 0 { s.scored as f64 / s.legal as f64 } else { 0.0 };
        let mean_gap = if s.scored > 0 { s.gap_sum / s.scored as f64 } else { 0.0 };
        let best_rank = s.best_rank.map_or("n/a".to_string(), |r| r.to_string());
        println!(
            "{name:<6} {:>8} {:>8} {:>8}   {legal_rate:>6.3} {scored_rate:>6.3}   {mean_gap:>10.4} {best_rank:>9}",
            s.seen, s.legal, s.scored
        );
    }
    println!();
    let mut combined = CardStats::default();
    for s in &totals {
        combined.merge(s);
    }
    let legal_rate = if combined.seen > 0 { combined.legal as f64 / combined.seen as f64 } else { 0.0 };
    let scored_rate = if combined.legal > 0 { combined.scored as f64 / combined.legal as f64 } else { 0.0 };
    let mean_rank = if combined.scored > 0 { combined.rank_sum as f64 / combined.scored as f64 } else { 0.0 };
    let mean_gap = if combined.scored > 0 { combined.gap_sum / combined.scored as f64 } else { 0.0 };
    println!(
        "combined: seen={} legal={} ({:.3}) scored={} ({:.3}) mean_rank={:.2} mean_gap={:.4} best_rank={}",
        combined.seen,
        combined.legal,
        legal_rate,
        combined.scored,
        scored_rate,
        mean_rank,
        mean_gap,
        combined.best_rank.map_or("n/a".to_string(), |r| r.to_string())
    );

    println!();
    println!("Winning move kind at every SCORED Mine-take decision point (what beat it):");
    let mut kinds: Vec<(&String, &u64)> = diag.winner_kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let kind_total: u64 = diag.winner_kinds.values().sum();
    for (kind, count) in &kinds {
        let rate = if kind_total > 0 { **count as f64 / kind_total as f64 } else { 0.0 };
        println!("  {kind:<14} {count:>6}  ({rate:.3})");
    }

    // Rank WeightKey terms by mean |contribution| to the winner-vs-take gap,
    // WITHOUT printing the raw contribution or any weight value -- only the
    // ranked key NAMES and the SIGN of their net effect (see `Diag`'s own
    // doc comment). `favors_winner` (positive mean) widens the gap against
    // the Mine take; `favors_take` (negative mean) narrows it.
    println!();
    println!(
        "Top WeightKey terms by mean |contribution| to the winner-vs-Mine-take gap (n={}):",
        diag.key_contrib_n
    );
    if diag.key_contrib_n > 0 {
        let n = diag.key_contrib_n as f64;
        let mut ranked: Vec<(WeightKey, f64)> =
            WeightKey::ALL.iter().enumerate().map(|(i, &k)| (k, diag.key_contrib_sum[i] / n)).collect();
        ranked.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (key, mean)) in ranked.iter().take(15).enumerate() {
            let sign = if *mean > 0.0 { "favors_winner" } else if *mean < 0.0 { "favors_take" } else { "neutral" };
            println!("  {:>2}. {:<24} {sign}", rank + 1, format!("{key:?}"));
        }
    } else {
        println!("  (no scored Mine-take occurrences to rank)");
    }

    ExitCode::SUCCESS
}
