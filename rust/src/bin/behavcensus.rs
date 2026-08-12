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

use tta::bots::greedy::{build_bots, BotKind, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::effects;
use tta::game::{self, MOVE_CAP};
use tta::moves::Move;
use tta::{Age, CardId, CardType};

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

    // ---- wonders ----
    wonders_completed_per_playergame: Vec<i32>,
    wonders_started_per_playergame: Vec<i32>,
    wonders_abandoned_per_playergame: Vec<i32>,
    wonder_completed_name_counts: HashMap<&'static str, u64>,

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

        self.wonders_completed_per_playergame.extend(other.wonders_completed_per_playergame);
        self.wonders_started_per_playergame.extend(other.wonders_started_per_playergame);
        self.wonders_abandoned_per_playergame.extend(other.wonders_abandoned_per_playergame);
        merge_map(&mut self.wonder_completed_name_counts, other.wonder_completed_name_counts);

        merge_map(&mut self.final_government, other.final_government);
        self.gov_change_count_per_playergame.extend(other.gov_change_count_per_playergame);
        self.first_gov_change_round.extend(other.first_gov_change_round);
        self.n_stayed_despotism += other.n_stayed_despotism;

        self.wars_declared_per_playergame.extend(other.wars_declared_per_playergame);
        self.aggressions_played_per_playergame.extend(other.aggressions_played_per_playergame);
        self.n_weakest_at_end_age2 += other.n_weakest_at_end_age2;
        self.n_playergames_with_age2_sample += other.n_playergames_with_age2_sample;

        for i in 0..5 {
            self.age_samples[i].extend(other.age_samples[i].drain(..));
        }
        self.final_score.extend(other.final_score);
    }
}

fn merge_map(a: &mut HashMap<&'static str, u64>, b: HashMap<&'static str, u64>) {
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
    gov_changes: u32,
    first_gov_change_round: Option<u16>,
    wars_declared: u32,
    aggressions: u32,
}

impl PlayerTrack {
    fn new() -> Self {
        PlayerTrack {
            first_take: None,
            first_develop: None,
            first_wonder_round: None,
            wonder_started: HashMap::new(),
            gov_changes: 0,
            first_gov_change_round: None,
            wars_declared: 0,
            aggressions: 0,
        }
    }
}

fn play_one(players: u8, weights: Weights, seed: u64) -> (Report, bool) {
    let seats: Vec<Seat> = (0..players).map(|_| Seat { kind: BotKind::Weighted, weights }).collect();
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
            _ => None,
        };
        let pre_govt = state.players[actor as usize].government;
        let pre_age = state.age_civil;

        // pre-move snapshot for every player, used only if this move is the
        // one that advances state.age_civil.
        let pre_snapshots: Vec<AgeSample> = (0..players).map(|i| sample_player(&state, i)).collect();

        game::step(&mut state, mv);
        moves_played += 1;

        // ---- openings ----
        if let Some(card) = taken_card {
            if !card.is_none() && pre_age == Age::A && tracks[actor as usize].first_take.is_none() {
                tracks[actor as usize].first_take = Some(card);
            }
        }
        if let Move::Develop { card } = mv {
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

    for i in 0..players {
        let p = &state.players[i as usize];
        report.n_player_games += 1;

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
        let started = &tracks[i as usize].wonder_started;
        report.wonders_completed_per_playergame.push(completed.len() as i32);
        report.wonders_started_per_playergame.push(started.len() as i32);
        report
            .wonders_abandoned_per_playergame
            .push((started.len() as i32 - completed.len() as i32).max(0));
        for c in &completed {
            *report.wonder_completed_name_counts.entry(c.name()).or_insert(0) += 1;
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
    fn merge_combines_two_reports_games_and_score_vectors() {
        let mut a = Report::default();
        a.games = 1;
        a.final_score.push(10);
        let mut b = Report::default();
        b.games = 1;
        b.final_score.push(20);
        a.merge(b);
        assert_eq!(a.games, 2);
        assert_eq!(a.final_score, vec![10, 20]);
    }
}
