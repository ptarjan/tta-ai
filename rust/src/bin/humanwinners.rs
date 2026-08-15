//! `humanwinners` -- winner-vs-loser evidence for a human-strategy document,
//! mined from the 1,011-game BGO human corpus (`sources/bgo/`), base game
//! only. Answers 8 questions (wonders, government, military, aggression/war,
//! culture/science rates, leaders, tempo, comebacks), split by player count,
//! ALWAYS with a sample size next to every number.
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals   # once
//! cargo run --profile difftest --bin humanwinners -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals > /tmp/humanwinners_out.md
//! ```
//!
//! # Method: two passes, first is text-only, second is the one place engine
//! replay is used
//!
//! Pass 1 (`parse_game`) reads every journal line once and extracts
//! everything a human player at the table could see directly from text:
//! winner/rank per colour (the "WINNER IS ... AS <COLOR>" line), completed
//! wonders, government changes, leaders elected, war/aggression declarations
//! (and their targets), per-age cumulative culture/science ("... (now N)"),
//! and the post-game "Impact of Strength" scoring (a text-only proxy for
//! military rank that needs no engine replay at all). This is ~100% coverage
//! by construction -- no dependency on `tta::replay_common::replay_game`
//! completing.
//!
//! Pass 2 (`run_military_pass`) is the ONE place this binary drives the real
//! engine (`replay_game(..., record_decisions: true)`), and ONLY to answer
//! "what was each player's military-strength RANK at the end of Age II /
//! Age III" -- something the journal text does not carry as a running total
//! (only sparse per-combat "X strength: N" lines, not present every round).
//! Coverage here is reported honestly and is expected to be well under 1011
//! (`docs/REPLAY.md`'s own caveat: the reconstruction does not complete most
//! games) -- Q3 also answers a strictly-text version (the Impact-of-Strength
//! based "who is the military leader" question) that does not depend on this
//! pass at all, so a low coverage number here does not block Q3's headline.
//!
//! # Legality
//!
//! Every field this file reads is something a human player at the table
//! could see: journal text (public), `index.tsv`'s own results column, and
//! (for Pass 2) `effects::army_strength` computed from PUBLIC tableau state
//! (`PlayerState.techs`/`tactic`) reconstructed from public actions -- never
//! a rival's hidden military-hand contents or unshuffled deck order.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{
    actor_and_rest, build_card_index, classify, find_ascii_ci, leading_int, parse_index,
    parse_winner_line, title_case, ActionClass, Color, GameMeta, LineOutcome,
};
use tta::effects::army_strength;
use tta::replay_common::replay_game;
use tta::{Age, CardId};

// ---------------------------------------------------------------------
// Small parsing helpers over raw journal text (all ASCII-safe: never slice
// a lowercased copy of text that might contain non-ASCII, only ever slice
// the ORIGINAL string at byte offsets found by scanning ASCII markers).
// `title_case`/`leading_int`/`find_ascii_ci`/`parse_winner_line` moved to
// `tta::corpus` (2026-08-13): `bin/humanopenings.rs`'s own text-based
// outcome determination needs `parse_winner_line` too -- see that module's
// doc for why text beats gating outcome on full engine-replay completion --
// imported above under their original names so nothing below this point
// changes.
// ---------------------------------------------------------------------

/// Given `"... <marker><ColorName> ..."`, returns the colour named right
/// after `marker` (case-insensitive marker, e.g. `" on "` for a war
/// declaration's target, `" against "` for an aggression's target).
fn color_after(text: &str, marker: &str) -> Option<Color> {
    let pos = find_ascii_ci(text, marker)?;
    let after = &text[pos + marker.len()..];
    let word: String = after.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    Color::parse(&title_case(&word))
}

/// `"<N> <label> (now <M>)"` -> `M`. Used for the per-turn `scores:` line's
/// cumulative culture/science totals.
fn extract_now_value(text: &str, label: &str) -> Option<i32> {
    let pos = text.find(label)?;
    let after = &text[pos + label.len()..];
    let end = after.find(')')?;
    after[..end].trim().parse().ok()
}

fn parse_age(s: &str) -> Option<Age> {
    match s {
        "A" => Some(Age::A),
        "I" => Some(Age::I),
        "II" => Some(Age::II),
        "III" => Some(Age::III),
        "IV" => Some(Age::IV),
        _ => None,
    }
}

fn age_idx(a: Age) -> usize {
    match a {
        Age::A => 0,
        Age::I => 1,
        Age::II => 2,
        Age::III => 3,
        Age::IV => 4,
    }
}

/// BGO's fixed seating convention (confirmed by `Color::seat`'s own doc
/// comment, re-confirmed here empirically over the corpus: 2p is always
/// Orange/Purple, 3p always Orange/Purple/Green, 4p all four) -- the inverse
/// of `Color::seat`. Matches `replay_common.rs`'s own private
/// `target_actor_color` (same shape, same wildcard-for-u8 reason: `u8` can't
/// be exhaustively matched).
fn color_for_seat(seat: u8) -> Color {
    match seat {
        0 => Color::Orange,
        1 => Color::Purple,
        2 => Color::Green,
        _ => Color::Grey,
    }
}

// `parse_winner_line` moved to `tta::corpus` (imported above) -- see this
// file's own top-of-file note.

// ---------------------------------------------------------------------
// Per-(game, colour) aggregate -- everything Pass 1 extracts from text.
// ---------------------------------------------------------------------

#[derive(Clone)]
struct PlayerAgg {
    // Stored for symmetry with the constructor's `Color` argument and kept
    // on the struct for future per-player reporting; today every reader
    // gets the colour from the `by_color: HashMap<Color, PlayerAgg>` key
    // instead, so this field itself is never read. Removing it would also
    // mean dropping the `new(color: Color)` parameter and touching every
    // call site for a field that costs nothing to keep -- not worth the
    // extra churn for a lint-only fix.
    #[allow(dead_code)]
    color: Color,
    /// 1-based; 0 means the "WINNER IS" line wasn't found/parsed for this
    /// game (excluded from every winner/loser split, counted in coverage).
    rank: u8,
    score: i32,
    /// Completed wonders (`"; ; Wonder completed"` suffix seen), with the
    /// age at completion.
    wonders: Vec<(Age, CardId)>,
    /// Actual government-change events (`"revolutions Change government to
    /// X"`), in order: (round, age, government).
    gov_changes: Vec<(u32, Age, CardId)>,
    leaders: Vec<CardId>,
    action_counts: HashMap<ActionClass, u32>,
    war_declares: Vec<(Age, Color)>,
    aggression_plays: Vec<(Age, Color)>,
    /// Cumulative culture/science AT THE END of each age (last `(now N)`
    /// value seen while the line's own age column was that age), indexed by
    /// `age_idx`. `age_round` is the shared game-round of that last line.
    age_culture: [Option<i32>; 5],
    age_science: [Option<i32>; 5],
    age_round: [Option<u32>; 5],
    /// This colour's rank (1 = strongest) among the table on cumulative
    /// culture at that age-end -- filled in a per-game post-pass once every
    /// colour's `age_culture` is known.
    age_culture_rank: [Option<u8>; 5],
    age_culture_delta_from_mean: [Option<f64>; 5],
    /// Culture this colour was granted by the final "Impact of Strength"
    /// scoring-event line, if that event fired this game (`None` if the
    /// event never fired; `Some(0)` if it fired and this colour scored
    /// nothing from it, i.e. was not the/a military leader).
    strength_score: Option<i32>,
    /// Filled by Pass 2 (engine replay) -- military-strength rank (1 =
    /// strongest) at the end of Age II / III. `None` if the replay never
    /// reached that far.
    age2_mil_rank: Option<u8>,
    age3_mil_rank: Option<u8>,
}

impl PlayerAgg {
    fn new(color: Color) -> Self {
        PlayerAgg {
            color,
            rank: 0,
            score: 0,
            wonders: Vec::new(),
            gov_changes: Vec::new(),
            leaders: Vec::new(),
            action_counts: HashMap::new(),
            war_declares: Vec::new(),
            aggression_plays: Vec::new(),
            age_culture: [None; 5],
            age_science: [None; 5],
            age_round: [None; 5],
            age_culture_rank: [None; 5],
            age_culture_delta_from_mean: [None; 5],
            strength_score: None,
            age2_mil_rank: None,
            age3_mil_rank: None,
        }
    }

    fn count(&self, c: ActionClass) -> u32 {
        self.action_counts.get(&c).copied().unwrap_or(0)
    }
}

struct GameAgg {
    id: String,
    players: u8,
    winner_parsed: bool,
    has_strength_line: bool,
    colors: Vec<Color>,
    by_color: HashMap<Color, PlayerAgg>,
}

// ---------------------------------------------------------------------
// Pass 1: text-only extraction, one game.
// ---------------------------------------------------------------------

fn parse_game(meta: &GameMeta, text: &str, card_index: &HashMap<&'static str, CardId>) -> GameAgg {
    let all_colors = [Color::Orange, Color::Purple, Color::Green, Color::Grey];
    let colors: Vec<Color> = all_colors[..meta.players as usize].to_vec();
    let mut by_color: HashMap<Color, PlayerAgg> = HashMap::new();
    for &c in &colors {
        by_color.insert(c, PlayerAgg::new(c));
    }

    let mut winner_parsed = false;
    let mut has_strength_line = false;

    for (lineno, line) in text.lines().enumerate() {
        if lineno == 0 || line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.splitn(5, '\t').collect();
        if fields.len() != 5 {
            continue;
        }
        let col2 = Color::parse(fields[1]);
        let age_field = fields[2];
        let round_field: u32 = fields[3].parse().unwrap_or(0);
        let raw_text = fields[4];
        let line_age = parse_age(age_field);

        if !winner_parsed {
            if let Some(pos) = raw_text.find("WINNER IS") {
                let ranks = parse_winner_line(&raw_text[pos..]);
                if !ranks.is_empty() {
                    for (rank, color, score) in ranks {
                        if let Some(p) = by_color.get_mut(&color) {
                            p.rank = rank;
                            p.score = score;
                        }
                    }
                    winner_parsed = true;
                }
            }
        }

        if raw_text.starts_with("Impact of Strength") {
            has_strength_line = true;
            for &c in &colors {
                if let Some(p) = by_color.get_mut(&c) {
                    if p.strength_score.is_none() {
                        p.strength_score = Some(0);
                    }
                }
            }
            for &c in &colors {
                let marker = format!("{} scores ", c.as_str());
                if let Some(pos2) = raw_text.find(&marker) {
                    if let Some(n) = leading_int(&raw_text[pos2 + marker.len()..]) {
                        if let Some(p) = by_color.get_mut(&c) {
                            p.strength_score = Some(n);
                        }
                    }
                }
            }
        }

        let LineOutcome::Action(c) = classify(card_index, raw_text) else {
            continue;
        };
        let actor = actor_and_rest(raw_text).map(|(col, _)| col).or(col2);
        let Some(actor) = actor else { continue };
        let Some(p) = by_color.get_mut(&actor) else { continue };
        *p.action_counts.entry(c.class).or_insert(0) += 1;

        match c.class {
            ActionClass::TakeCard => {}
            ActionClass::BuildBuilding => {}
            ActionClass::BuildUnit => {}
            ActionClass::BuildWonderStage => {
                if raw_text.contains("Wonder completed") {
                    if let (Some(id), Some(age)) = (c.card, line_age) {
                        p.wonders.push((age, id));
                    }
                }
            }
            ActionClass::IncreasePopulation => {}
            ActionClass::UpgradeUnit => {}
            ActionClass::UpgradeProduction => {}
            ActionClass::DevelopTechnology => {}
            ActionClass::ElectLeader => {
                if let Some(id) = c.card {
                    p.leaders.push(id);
                }
            }
            ActionClass::ChangeGovernment => {
                if let (Some(id), Some(age)) = (c.card, line_age) {
                    p.gov_changes.push((round_field, age, id));
                }
            }
            ActionClass::PlayTactic => {}
            ActionClass::DeclareWar => {
                if let (Some(target), Some(age)) = (color_after(raw_text, " on "), line_age) {
                    p.war_declares.push((age, target));
                }
            }
            ActionClass::WinWar => {}
            ActionClass::PlayAggression => {
                if let (Some(target), Some(age)) = (color_after(raw_text, " against "), line_age) {
                    p.aggression_plays.push((age, target));
                }
            }
            ActionClass::ProposePact => {}
            ActionClass::AcceptPact => {}
            ActionClass::Colonize => {}
            ActionClass::Discard => {}
            ActionClass::Bid => {}
            ActionClass::WinAuction => {}
            ActionClass::Destroy => {}
            ActionClass::Disband => {}
            ActionClass::Pass => {}
            ActionClass::PlayEvent => {}
            ActionClass::PlayActionCard => {}
            ActionClass::PutBack => {}
            ActionClass::EndTurn => {
                if let Some(age) = line_age {
                    let idx = age_idx(age);
                    if let Some(cult) = extract_now_value(raw_text, "culture (now ") {
                        p.age_culture[idx] = Some(cult);
                        p.age_round[idx] = Some(round_field);
                    }
                    if let Some(sci) = extract_now_value(raw_text, "science (now ") {
                        p.age_science[idx] = Some(sci);
                    }
                }
            }
            ActionClass::RemoveLeaderYellow => {}
            ActionClass::ColumbusColonize => {}
            ActionClass::Barbarossa => {}
            ActionClass::BachTheater => {}
        }
    }

    // Table-relative culture: rank + delta-from-mean at each age-end, once
    // every colour's cumulative value for that age is known.
    for idx in 0..5 {
        let mut vals: Vec<(Color, i32)> = Vec::new();
        for &c in &colors {
            if let Some(v) = by_color.get(&c).and_then(|p| p.age_culture[idx]) {
                vals.push((c, v));
            }
        }
        if vals.is_empty() {
            continue;
        }
        let mean = vals.iter().map(|(_, v)| *v as f64).sum::<f64>() / vals.len() as f64;
        let mut sorted = vals.clone();
        sorted.sort_by_key(|x| std::cmp::Reverse(x.1));
        for (rank_idx, (color, _)) in sorted.iter().enumerate() {
            if let Some(p) = by_color.get_mut(color) {
                p.age_culture_rank[idx] = Some((rank_idx + 1) as u8);
            }
        }
        for (color, v) in &vals {
            if let Some(p) = by_color.get_mut(color) {
                p.age_culture_delta_from_mean[idx] = Some(*v as f64 - mean);
            }
        }
    }

    GameAgg {
        id: meta.id.clone(),
        players: meta.players,
        winner_parsed,
        has_strength_line,
        colors,
        by_color,
    }
}

// ---------------------------------------------------------------------
// Pass 2: engine replay, ONLY for military rank at end of Age II / III.
// ---------------------------------------------------------------------

struct MilitaryRanks {
    age2: HashMap<(String, Color), u8>,
    age3: HashMap<(String, Color), u8>,
    games_attempted: u32,
    games_reached_age2: u32,
    games_reached_age3: u32,
}

fn run_military_pass(
    games: &[GameMeta],
    journals_dir: &str,
    card_index: &HashMap<&'static str, CardId>,
) -> MilitaryRanks {
    let mut age2: HashMap<(String, Color), u8> = HashMap::new();
    let mut age3: HashMap<(String, Color), u8> = HashMap::new();
    let mut games_attempted = 0u32;
    let mut games_reached_age2 = 0u32;
    let mut games_reached_age3 = 0u32;

    for meta in games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        games_attempted += 1;
        let result = replay_game(meta, &text, card_index, true);

        let mut last_age2_idx: Option<usize> = None;
        let mut last_age3_idx: Option<usize> = None;
        for (i, d) in result.decisions.iter().enumerate() {
            match d.state.age_civil {
                Age::II => last_age2_idx = Some(i),
                Age::III => last_age3_idx = Some(i),
                Age::A | Age::I | Age::IV => {}
            }
        }

        if let Some(i) = last_age2_idx {
            games_reached_age2 += 1;
            let state = &result.decisions[i].state;
            let mut strengths: Vec<(u8, i32)> =
                (0..meta.players).map(|seat| (seat, army_strength(&state.players[seat as usize]))).collect();
            strengths.sort_by_key(|x| std::cmp::Reverse(x.1));
            for (rank_idx, (seat, _)) in strengths.iter().enumerate() {
                age2.insert((meta.id.clone(), color_for_seat(*seat)), (rank_idx + 1) as u8);
            }
        }
        if let Some(i) = last_age3_idx {
            games_reached_age3 += 1;
            let state = &result.decisions[i].state;
            let mut strengths: Vec<(u8, i32)> =
                (0..meta.players).map(|seat| (seat, army_strength(&state.players[seat as usize]))).collect();
            strengths.sort_by_key(|x| std::cmp::Reverse(x.1));
            for (rank_idx, (seat, _)) in strengths.iter().enumerate() {
                age3.insert((meta.id.clone(), color_for_seat(*seat)), (rank_idx + 1) as u8);
            }
        }
    }

    MilitaryRanks { age2, age3, games_attempted, games_reached_age2, games_reached_age3 }
}

// ---------------------------------------------------------------------
// Reporting helpers
// ---------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct MeanStat {
    n: u64,
    sum: f64,
}
impl MeanStat {
    fn add(&mut self, v: f64) {
        self.n += 1;
        self.sum += v;
    }
    fn mean(&self) -> f64 {
        if self.n == 0 {
            f64::NAN
        } else {
            self.sum / self.n as f64
        }
    }
}

#[derive(Default, Clone, Copy)]
struct RateStat {
    n: u64,
    hits: u64,
}
impl RateStat {
    fn add(&mut self, hit: bool) {
        self.n += 1;
        if hit {
            self.hits += 1;
        }
    }
    fn rate(&self) -> f64 {
        if self.n == 0 {
            f64::NAN
        } else {
            self.hits as f64 / self.n as f64
        }
    }
}

fn fmt_mean(s: &MeanStat) -> String {
    if s.n == 0 {
        "n/a (n=0)".to_string()
    } else {
        format!("{:.3} (n={})", s.mean(), s.n)
    }
}
fn fmt_rate(s: &RateStat) -> String {
    if s.n == 0 {
        "n/a (n=0)".to_string()
    } else {
        format!("{:.1}% (n={})", s.rate() * 100.0, s.n)
    }
}
fn lowconf(n: u64) -> &'static str {
    if n < 30 {
        " [LOW CONFIDENCE, n<30]"
    } else {
        ""
    }
}

// ---------------------------------------------------------------------
// main
// ---------------------------------------------------------------------

fn run(index_path: &str, journals_dir: &str) -> Result<(), String> {
    let card_index = build_card_index();
    let games = parse_index(index_path)?;
    println!("Parsed {} games from {index_path}.\n", games.len());

    // -------------------- Pass 1 --------------------
    let mut all_games: Vec<GameAgg> = Vec::new();
    let mut no_journal: u32 = 0;
    for meta in &games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(_) => {
                no_journal += 1;
                continue;
            }
        };
        all_games.push(parse_game(meta, &text, &card_index));
    }
    let winner_parsed_n = all_games.iter().filter(|g| g.winner_parsed).count();
    let strength_line_n = all_games.iter().filter(|g| g.has_strength_line).count();
    println!(
        "## Coverage\n\n- {} games indexed, {} journal files missing, {} journals parsed.\n\
         - Winner/rank line (\"WINNER IS ... AS <COLOR>\") parsed: {}/{} games ({:.1}%).\n\
         - \"Impact of Strength\" scoring line present (post-game military-leader signal, no replay needed): {}/{} games ({:.1}%).\n",
        games.len(),
        no_journal,
        all_games.len(),
        winner_parsed_n,
        all_games.len(),
        winner_parsed_n as f64 * 100.0 / all_games.len().max(1) as f64,
        strength_line_n,
        all_games.len(),
        strength_line_n as f64 * 100.0 / all_games.len().max(1) as f64
    );

    // -------------------- Pass 2 (engine replay, military only) --------------------
    let mil = run_military_pass(&games, journals_dir, &card_index);
    println!(
        "- Engine-replay pass (military rank at Age II/III end): attempted {} games, \
         reached an Age-II decision in {} ({:.1}%), reached an Age-III decision in {} ({:.1}%). \
         Per `docs/REPLAY.md`, full replay does not complete most human games; this is the honest \
         coverage, not a target to hit.\n",
        mil.games_attempted,
        mil.games_reached_age2,
        mil.games_reached_age2 as f64 * 100.0 / mil.games_attempted.max(1) as f64,
        mil.games_reached_age3,
        mil.games_reached_age3 as f64 * 100.0 / mil.games_attempted.max(1) as f64
    );

    // Flat list of (players, PlayerAgg) for every ranked seat, joined with
    // Pass 2's military ranks.
    struct Rec {
        players: u8,
        p: PlayerAgg,
    }
    let mut recs: Vec<Rec> = Vec::new();
    for g in &all_games {
        for &c in &g.colors {
            let mut p = g.by_color.get(&c).expect("colour present").clone();
            p.age2_mil_rank = mil.age2.get(&(g.id.clone(), c)).copied();
            p.age3_mil_rank = mil.age3.get(&(g.id.clone(), c)).copied();
            recs.push(Rec { players: g.players, p });
        }
    }
    let ranked = |players: u8| -> Vec<&Rec> {
        recs.iter().filter(|r| r.players == players && r.p.rank > 0).collect()
    };
    let winners = |players: u8| -> Vec<&Rec> {
        recs.iter().filter(|r| r.players == players && r.p.rank == 1).collect()
    };
    let losers = |players: u8| -> Vec<&Rec> {
        recs.iter().filter(|r| r.players == players && r.p.rank > 1).collect()
    };

    for &players in &[2u8, 3u8, 4u8] {
        let games_n = all_games.iter().filter(|g| g.players == players && g.winner_parsed).count();
        let w = winners(players);
        let l = losers(players);
        println!(
            "\n# {}p ({} games with a parsed winner, {} winner-seats, {} loser-seats)\n",
            players,
            games_n,
            w.len(),
            l.len()
        );

        // ---- Q1: wonders ----
        println!("## Q1: Wonders\n");
        let mut wonders_w = MeanStat::default();
        let mut wonders_l = MeanStat::default();
        for r in &w {
            wonders_w.add(r.p.wonders.len() as f64);
        }
        for r in &l {
            wonders_l.add(r.p.wonders.len() as f64);
        }
        println!("Mean wonders completed: winners {} vs losers {}\n", fmt_mean(&wonders_w), fmt_mean(&wonders_l));

        let mut by_wonder: HashMap<&'static str, RateStat> = HashMap::new();
        for r in ranked(players) {
            let mut seen: Vec<&'static str> = r.p.wonders.iter().map(|(_, id)| id.get().base_name).collect();
            seen.sort_unstable();
            seen.dedup();
            for name in seen {
                by_wonder.entry(name).or_default().add(r.p.rank == 1);
            }
        }
        let baseline = RateStat { n: ranked(players).len() as u64, hits: w.len() as u64 };
        println!(
            "Baseline win rate this bucket: {}\n",
            fmt_rate(&baseline)
        );
        println!("| wonder | n completed (seats) | win rate among completers |");
        println!("|---|---|---|");
        let mut wnames: Vec<&&'static str> = by_wonder.keys().collect();
        wnames.sort_by(|a, b| by_wonder[*b].n.cmp(&by_wonder[*a].n));
        for name in wnames {
            let s = by_wonder[name];
            println!("| {name} | {} | {}{} |", s.n, fmt_rate(&s), lowconf(s.n));
        }

        // ---- Q2: government ----
        println!("\n## Q2: Government\n");
        let mut final_gov_w: HashMap<&'static str, u32> = HashMap::new();
        let mut final_gov_l: HashMap<&'static str, u32> = HashMap::new();
        let mut first_change_round_w = MeanStat::default();
        let mut first_change_round_l = MeanStat::default();
        let mut never_changed_w = RateStat::default();
        let mut never_changed_l = RateStat::default();
        for r in &w {
            let name = r.p.gov_changes.last().map(|(_, _, id)| id.get().base_name).unwrap_or("Despotism (never changed)");
            *final_gov_w.entry(name).or_insert(0) += 1;
            if let Some((round, _, _)) = r.p.gov_changes.first() {
                first_change_round_w.add(*round as f64);
            }
            never_changed_w.add(r.p.gov_changes.is_empty());
        }
        for r in &l {
            let name = r.p.gov_changes.last().map(|(_, _, id)| id.get().base_name).unwrap_or("Despotism (never changed)");
            *final_gov_l.entry(name).or_insert(0) += 1;
            if let Some((round, _, _)) = r.p.gov_changes.first() {
                first_change_round_l.add(*round as f64);
            }
            never_changed_l.add(r.p.gov_changes.is_empty());
        }
        println!("| final government | winners (n) | losers (n) |");
        println!("|---|---|---|");
        let mut gov_names: Vec<&&'static str> = final_gov_w.keys().chain(final_gov_l.keys()).collect();
        gov_names.sort_unstable();
        gov_names.dedup();
        for name in gov_names {
            println!(
                "| {name} | {} | {} |",
                final_gov_w.get(name).copied().unwrap_or(0),
                final_gov_l.get(name).copied().unwrap_or(0)
            );
        }
        println!(
            "\nRound of first government change (leaving Despotism): winners {} vs losers {}\n",
            fmt_mean(&first_change_round_w),
            fmt_mean(&first_change_round_l)
        );
        println!(
            "Never changed government (stayed Despotism) rate: winners {} vs losers {}\n",
            fmt_rate(&never_changed_w),
            fmt_rate(&never_changed_l)
        );

        // ---- Q3: military ----
        println!("## Q3: Military\n");
        let mut age2_rank_w = MeanStat::default();
        let mut age2_rank_l = MeanStat::default();
        let mut age3_rank_w = MeanStat::default();
        let mut age3_rank_l = MeanStat::default();
        for r in &w {
            if let Some(v) = r.p.age2_mil_rank {
                age2_rank_w.add(v as f64);
            }
            if let Some(v) = r.p.age3_mil_rank {
                age3_rank_w.add(v as f64);
            }
        }
        for r in &l {
            if let Some(v) = r.p.age2_mil_rank {
                age2_rank_l.add(v as f64);
            }
            if let Some(v) = r.p.age3_mil_rank {
                age3_rank_l.add(v as f64);
            }
        }
        println!(
            "Engine-replay military rank (1=strongest), Age II end: winners {} vs losers {}\n",
            fmt_mean(&age2_rank_w),
            fmt_mean(&age2_rank_l)
        );
        println!(
            "Engine-replay military rank (1=strongest), Age III end: winners {} vs losers {}\n",
            fmt_mean(&age3_rank_w),
            fmt_mean(&age3_rank_l)
        );
        // Impact-of-Strength text-only proxy: no replay needed.
        let mut strength_leader_wins = RateStat::default();
        let mut strength_last_wins = RateStat::default();
        for g in all_games.iter().filter(|g| g.players == players && g.winner_parsed && g.has_strength_line) {
            let mut scored: Vec<(Color, i32)> = g
                .colors
                .iter()
                .filter_map(|&c| g.by_color.get(&c).and_then(|p| p.strength_score).map(|s| (c, s)))
                .collect();
            if scored.len() != g.players as usize {
                continue;
            }
            scored.sort_by_key(|x| std::cmp::Reverse(x.1));
            let leader = scored[0].0;
            let last = scored[scored.len() - 1].0;
            let leader_won = g.by_color.get(&leader).map(|p| p.rank == 1).unwrap_or(false);
            let last_won = g.by_color.get(&last).map(|p| p.rank == 1).unwrap_or(false);
            strength_leader_wins.add(leader_won);
            strength_last_wins.add(last_won);
        }
        println!(
            "Impact-of-Strength military leader also overall winner (text-only, no replay): {}\n",
            fmt_rate(&strength_leader_wins)
        );
        println!(
            "Win rate for the Impact-of-Strength LAST-place military player: {}\n",
            fmt_rate(&strength_last_wins)
        );

        // ---- Q4: aggression and war ----
        println!("## Q4: Aggression and war\n");
        let mut war_w = RateStat::default();
        let mut war_l = RateStat::default();
        let mut agg_w = RateStat::default();
        let mut agg_l = RateStat::default();
        for r in &w {
            war_w.add(r.p.count(ActionClass::DeclareWar) > 0);
            agg_w.add(r.p.count(ActionClass::PlayAggression) > 0);
        }
        for r in &l {
            war_l.add(r.p.count(ActionClass::DeclareWar) > 0);
            agg_l.add(r.p.count(ActionClass::PlayAggression) > 0);
        }
        println!(">=1 war declared: winners {} vs losers {}\n", fmt_rate(&war_w), fmt_rate(&war_l));
        println!(">=1 aggression played: winners {} vs losers {}\n", fmt_rate(&agg_w), fmt_rate(&agg_l));

        let mut aggressor_wins = RateStat::default();
        for g in all_games.iter().filter(|g| g.players == players && g.winner_parsed) {
            for &c in &g.colors {
                let p = g.by_color.get(&c).expect("colour present");
                if !p.war_declares.is_empty() || !p.aggression_plays.is_empty() {
                    aggressor_wins.add(p.rank == 1);
                }
            }
        }
        let table_base = RateStat { n: ranked(players).len() as u64, hits: w.len() as u64 };
        println!(
            "Win rate for a player who declared >=1 war OR played >=1 aggression: {} (table base rate: {})\n",
            fmt_rate(&aggressor_wins),
            fmt_rate(&table_base)
        );

        let mut target_wins = RateStat::default();
        for g in all_games.iter().filter(|g| g.players == players && g.winner_parsed) {
            let mut targets: Vec<Color> = Vec::new();
            for &c in &g.colors {
                let p = g.by_color.get(&c).expect("colour present");
                targets.extend(p.war_declares.iter().map(|(_, t)| *t));
                targets.extend(p.aggression_plays.iter().map(|(_, t)| *t));
            }
            targets.sort_by_key(|c| c.as_str());
            targets.dedup();
            for t in targets {
                if let Some(p) = g.by_color.get(&t) {
                    if p.rank > 0 {
                        target_wins.add(p.rank == 1);
                    }
                }
            }
        }
        println!(
            "Win rate for a player who was the TARGET of >=1 war/aggression: {} (table base rate: {})\n",
            fmt_rate(&target_wins),
            fmt_rate(&table_base)
        );

        // ---- Q5: culture / science rates ----
        println!("## Q5: Culture and science rates\n");
        println!("| age-end | metric | winners | losers |");
        println!("|---|---|---|---|");
        for (age, label) in [(Age::I, "I"), (Age::II, "II"), (Age::III, "III"), (Age::IV, "IV")] {
            let idx = age_idx(age);
            let mut cult_abs_w = MeanStat::default();
            let mut cult_abs_l = MeanStat::default();
            let mut cult_rank_w = MeanStat::default();
            let mut cult_rank_l = MeanStat::default();
            let mut cult_delta_w = MeanStat::default();
            let mut cult_delta_l = MeanStat::default();
            let mut sci_abs_w = MeanStat::default();
            let mut sci_abs_l = MeanStat::default();
            for r in &w {
                if let (Some(cult), Some(round)) = (r.p.age_culture[idx], r.p.age_round[idx]) {
                    if round > 0 {
                        cult_abs_w.add(cult as f64 / round as f64);
                    }
                }
                if let Some(rank) = r.p.age_culture_rank[idx] {
                    cult_rank_w.add(rank as f64);
                }
                if let Some(delta) = r.p.age_culture_delta_from_mean[idx] {
                    cult_delta_w.add(delta);
                }
                if let (Some(sci), Some(round)) = (r.p.age_science[idx], r.p.age_round[idx]) {
                    if round > 0 {
                        sci_abs_w.add(sci as f64 / round as f64);
                    }
                }
            }
            for r in &l {
                if let (Some(cult), Some(round)) = (r.p.age_culture[idx], r.p.age_round[idx]) {
                    if round > 0 {
                        cult_abs_l.add(cult as f64 / round as f64);
                    }
                }
                if let Some(rank) = r.p.age_culture_rank[idx] {
                    cult_rank_l.add(rank as f64);
                }
                if let Some(delta) = r.p.age_culture_delta_from_mean[idx] {
                    cult_delta_l.add(delta);
                }
                if let (Some(sci), Some(round)) = (r.p.age_science[idx], r.p.age_round[idx]) {
                    if round > 0 {
                        sci_abs_l.add(sci as f64 / round as f64);
                    }
                }
            }
            println!("| {label} | culture/round so far (absolute) | {} | {} |", fmt_mean(&cult_abs_w), fmt_mean(&cult_abs_l));
            println!("| {label} | culture RANK at table (1=highest) | {} | {} |", fmt_mean(&cult_rank_w), fmt_mean(&cult_rank_l));
            println!("| {label} | culture delta-from-table-mean | {} | {} |", fmt_mean(&cult_delta_w), fmt_mean(&cult_delta_l));
            println!("| {label} | science/round so far (absolute) | {} | {} |", fmt_mean(&sci_abs_w), fmt_mean(&sci_abs_l));
        }

        // ---- Q6: leaders ----
        println!("\n## Q6: Leaders\n");
        let mut leader_stat: HashMap<&'static str, RateStat> = HashMap::new();
        for r in ranked(players) {
            let mut seen: Vec<&'static str> = r.p.leaders.iter().map(|id| id.get().base_name).collect();
            seen.sort_unstable();
            seen.dedup();
            for name in seen {
                leader_stat.entry(name).or_default().add(r.p.rank == 1);
            }
        }
        println!("| leader | n taken (seats) | win rate when taken | table base rate |");
        println!("|---|---|---|---|");
        let mut lnames: Vec<&&'static str> = leader_stat.keys().collect();
        lnames.sort_by(|a, b| leader_stat[*b].n.cmp(&leader_stat[*a].n));
        for name in lnames {
            let s = leader_stat[name];
            println!("| {name} | {} | {}{} | {} |", s.n, fmt_rate(&s), lowconf(s.n), fmt_rate(&table_base));
        }

        // ---- Q7: tempo ----
        println!("\n## Q7: Tempo\n");
        let mut takes_w = MeanStat::default();
        let mut takes_l = MeanStat::default();
        let mut ratio_w = MeanStat::default();
        let mut ratio_l = MeanStat::default();
        let mut putback_w = MeanStat::default();
        let mut putback_l = MeanStat::default();
        for r in &w {
            let takes = r.p.count(ActionClass::TakeCard);
            let builds = r.p.count(ActionClass::BuildBuilding) + r.p.count(ActionClass::BuildUnit) + r.p.count(ActionClass::BuildWonderStage);
            takes_w.add(takes as f64);
            if takes > 0 {
                ratio_w.add(builds as f64 / takes as f64);
            }
            putback_w.add(r.p.count(ActionClass::PutBack) as f64);
        }
        for r in &l {
            let takes = r.p.count(ActionClass::TakeCard);
            let builds = r.p.count(ActionClass::BuildBuilding) + r.p.count(ActionClass::BuildUnit) + r.p.count(ActionClass::BuildWonderStage);
            takes_l.add(takes as f64);
            if takes > 0 {
                ratio_l.add(builds as f64 / takes as f64);
            }
            putback_l.add(r.p.count(ActionClass::PutBack) as f64);
        }
        println!("Cards taken from row /game: winners {} vs losers {}\n", fmt_mean(&takes_w), fmt_mean(&takes_l));
        println!("Build:Take ratio: winners {} vs losers {}\n", fmt_mean(&ratio_w), fmt_mean(&ratio_l));
        println!(
            "PutBack actions /game (take-back UPPER BOUND, see `corpuscensus.rs`'s own caveat -- not all \
             put-backs are same-turn undo of a take): winners {} vs losers {}\n",
            fmt_mean(&putback_w),
            fmt_mean(&putback_l)
        );

        // ---- Q8: comebacks ----
        println!("## Q8: Comebacks (behind on culture at Age II end)\n");
        let idx2 = age_idx(Age::II);
        let idx3 = age_idx(Age::III);
        let mut behind_win = RateStat::default();
        let mut cult_rate_change_won = MeanStat::default();
        let mut cult_rate_change_stayed = MeanStat::default();
        let mut wonders_age3_won = MeanStat::default();
        let mut wonders_age3_stayed = MeanStat::default();
        let mut war_age3_won = MeanStat::default();
        let mut war_age3_stayed = MeanStat::default();
        let mut gov_change_after_age2_won = RateStat::default();
        let mut gov_change_after_age2_stayed = RateStat::default();
        for r in ranked(players) {
            let Some(rank2) = r.p.age_culture_rank[idx2] else { continue };
            if rank2 == 1 {
                continue; // already leading at Age II end -- not a "behind" player
            }
            let won = r.p.rank == 1;
            behind_win.add(won);

            let rate2 = match (r.p.age_culture[idx2], r.p.age_round[idx2]) {
                (Some(c), Some(rd)) if rd > 0 => Some(c as f64 / rd as f64),
                _ => None,
            };
            let rate3_within = match (r.p.age_culture[idx2], r.p.age_round[idx2], r.p.age_culture[idx3], r.p.age_round[idx3]) {
                (Some(c2), Some(rd2), Some(c3), Some(rd3)) if rd3 > rd2 => Some((c3 - c2) as f64 / (rd3 - rd2) as f64),
                _ => None,
            };
            let wonders_a3 = r.p.wonders.iter().filter(|(age, _)| *age == Age::III).count() as f64;
            let war_a3 = r.p.war_declares.iter().filter(|(age, _)| *age == Age::III).count() as f64;
            let round2 = r.p.age_round[idx2];
            let gov_after = round2.is_some()
                && r.p.gov_changes.iter().any(|(round, _, _)| *round > round2.unwrap());

            if let (Some(r2), Some(r3)) = (rate2, rate3_within) {
                let delta = r3 - r2;
                if won {
                    cult_rate_change_won.add(delta);
                } else {
                    cult_rate_change_stayed.add(delta);
                }
            }
            if won {
                wonders_age3_won.add(wonders_a3);
                war_age3_won.add(war_a3);
                gov_change_after_age2_won.add(gov_after);
            } else {
                wonders_age3_stayed.add(wonders_a3);
                war_age3_stayed.add(war_a3);
                gov_change_after_age2_stayed.add(gov_after);
            }
        }
        println!(
            "Players NOT in 1st on cumulative culture at Age II end: win rate {}\n",
            fmt_rate(&behind_win)
        );
        println!(
            "Of those, Age III within-age culture-rate CHANGE (rate in III minus rate in II): \
             went on to WIN {} vs stayed non-winner {}\n",
            fmt_mean(&cult_rate_change_won),
            fmt_mean(&cult_rate_change_stayed)
        );
        println!(
            "Wonders completed IN Age III: went on to WIN {} vs stayed non-winner {}\n",
            fmt_mean(&wonders_age3_won),
            fmt_mean(&wonders_age3_stayed)
        );
        println!(
            "Wars declared IN Age III: went on to WIN {} vs stayed non-winner {}\n",
            fmt_mean(&war_age3_won),
            fmt_mean(&war_age3_stayed)
        );
        println!(
            "Changed government AFTER Age II ended: went on to WIN {} vs stayed non-winner {}\n",
            fmt_rate(&gov_change_after_age2_won),
            fmt_rate(&gov_change_after_age2_stayed)
        );
    }

    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() != 2 {
        eprintln!("usage: humanwinners <index.tsv> <journals_dir>");
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
