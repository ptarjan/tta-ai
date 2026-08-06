//! `corpuscensus` -- a play-rate census over the 1,011-game BGO human corpus
//! (`sources/bgo/`), which nothing in this repo reads (see `docs/HAZARDS.md`'s
//! "no external anchor" entry: the bot's only strength signal is beating its
//! own frozen ancestors). This does not fix that hazard -- it does not play
//! the corpus, score it, or compare it to the bot -- it answers a narrower,
//! cheaper question: **what do strong humans actually spend their turns on**,
//! by counting, not by reconstructing game state (which the corpus cannot
//! support anyway -- see the module doc below on the row-imputation gap).
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
//! cargo run --profile difftest --bin corpuscensus -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals
//! ```
//!
//! Prints a Markdown report to stdout; `docs/HUMAN_PLAY.md`'s census section
//! is that output, pasted in by hand (this binary does not write the doc
//! itself -- the doc also carries prose the binary has no business
//! generating).
//!
//! # Method: journal text is a small fixed set of BGO-generated shapes
//!
//! Parsing -- the shape classifier, the card-name dictionary, the BGO/engine
//! spelling aliases, `index.tsv` reading -- lives in `tta::corpus`, shared
//! with `rust/src/bin/replay.rs` (the game-state reconstruction spike, see
//! `docs/REPLAY.md`). See that module's doc comment for the method (longest-
//! known-card-prefix matching against a closed dictionary, why column 2
//! isn't trusted as the line's actor, the nine BGO/engine spelling
//! mismatches) -- this file only counts what `tta::corpus::classify`
//! returns, it does not re-derive any of that.
//!
//! # What this deliberately does not do
//!
//! No game-state reconstruction, no move legality, no hidden information
//! (military hand contents / discards are permanently counts-only in the
//! journal, never identities -- nothing to recover here). No "prepare event"
//! action class: it does not exist in the text. BGO logs only the
//! resolution (`"X plays event ..."`); Age cards headed for a future event
//! slot leave no journal line of their own, so [`ActionClass::PlayEvent`] is
//! the closest observable proxy, not a distinct preparation step, and this
//! file does not pretend otherwise. No take-back correction: `docs/
//! HUMAN_PLAY.md`'s existing analysis (Python, now deleted, findings kept)
//! found ~8% of raw "takes" are a human undoing their own take within the
//! same turn (`"X takes Y in hand" ... "X puts Y back in the row" X gets
//! back exactly what it spent`); detecting that reliably needs the
//! actions-spent/actions-refunded pair to match exactly, which is state
//! tracking, not counting, so it is out of scope here. [`ActionClass::
//! PutBack`] is counted as its own bucket instead -- an upper bound on
//! take-backs, reported next to the raw take count so nobody mistakes one
//! for the other.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{
    build_card_index, card_expected, classify, parse_index, ActionClass, Color, GameMeta,
    LineOutcome, Tier,
};

// ---------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------

#[derive(Default)]
struct Bucket {
    games: u32,
    turns: u64,
    rounds_sum: u64,
    age_iv_games: u32,
    score_sum: i64,
    score_n: u64,
    score_max: i32,
    action_counts: HashMap<ActionClass, u64>,
    games_with_war: u32,
    games_with_aggression: u32,
    games_with_pact: u32,
    games_with_tactic: u32,
}

impl Bucket {
    fn add_game(&mut self, meta: &GameMeta, game_actions: &HashMap<ActionClass, u64>, turns: u64) {
        self.games += 1;
        self.turns += turns;
        self.rounds_sum += meta.rounds as u64;
        if meta.reached_age_iv {
            self.age_iv_games += 1;
        }
        for &s in &meta.scores {
            self.score_sum += s as i64;
            self.score_n += 1;
            self.score_max = self.score_max.max(s);
        }
        for (&class, &n) in game_actions {
            *self.action_counts.entry(class).or_insert(0) += n;
        }
        let has = |c: ActionClass| game_actions.get(&c).copied().unwrap_or(0) > 0;
        if has(ActionClass::DeclareWar) {
            self.games_with_war += 1;
        }
        if has(ActionClass::PlayAggression) {
            self.games_with_aggression += 1;
        }
        if has(ActionClass::ProposePact) {
            self.games_with_pact += 1;
        }
        if has(ActionClass::PlayTactic) {
            self.games_with_tactic += 1;
        }
    }

    fn count(&self, c: ActionClass) -> u64 {
        self.action_counts.get(&c).copied().unwrap_or(0)
    }

    fn per_game(&self, c: ActionClass) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.count(c) as f64 / self.games as f64
        }
    }

    fn per_turn(&self, c: ActionClass) -> f64 {
        if self.turns == 0 {
            0.0
        } else {
            self.count(c) as f64 / self.turns as f64
        }
    }
}

fn print_action_table(name: &str, buckets: &[(&str, &Bucket)]) {
    println!("\n### {name}\n");
    print!("| action class |");
    for (label, _) in buckets {
        print!(" {label} /game | {label} /turn |");
    }
    println!();
    print!("|---|");
    for _ in buckets {
        print!("---|---|");
    }
    println!();
    for &class in ActionClass::ALL {
        print!("| {} |", class.label());
        for (_, b) in buckets {
            print!(" {:.3} | {:.4} |", b.per_game(class), b.per_turn(class));
        }
        println!();
    }
}

fn print_top_n(title: &str, freq: &HashMap<&'static str, u64>, n: usize) {
    let mut entries: Vec<(&&str, &u64)> = freq.iter().collect();
    entries.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let total: u64 = freq.values().sum();
    let top_sum: u64 = entries.iter().take(n).map(|(_, c)| **c).sum();
    println!("\n### {title}\n");
    println!("Total: {total}, distinct names: {}\n", entries.len());
    println!("| rank | name | count | % of total |");
    println!("|---|---|---|---|");
    for (i, (name, count)) in entries.iter().take(n).enumerate() {
        println!(
            "| {} | {name} | {count} | {:.2}% |",
            i + 1,
            **count as f64 * 100.0 / total.max(1) as f64
        );
    }
    let tail = total - top_sum;
    let tail_names = entries.len().saturating_sub(n);
    println!(
        "\nLong tail (rank {}+, {tail_names} names): {tail} ({:.2}%)",
        n + 1,
        tail as f64 * 100.0 / total.max(1) as f64
    );
}

// ---------------------------------------------------------------------
// main
// ---------------------------------------------------------------------

fn run(index_path: &str, journals_dir: &str) -> Result<(), String> {
    let card_index = build_card_index();
    let games = parse_index(index_path)?;
    println!("Parsed {} games from {index_path}.", games.len());

    let mut by_players: HashMap<u8, Bucket> = HashMap::new();
    let mut by_tier: HashMap<Tier, Bucket> = HashMap::new();
    let mut overall = Bucket::default();

    let mut takes_freq: HashMap<&'static str, u64> = HashMap::new();
    let mut plays_freq: HashMap<&'static str, u64> = HashMap::new();

    let mut total_lines: u64 = 0;
    let mut classified_lines: u64 = 0;
    let mut unclassified_shapes: HashMap<String, u64> = HashMap::new();

    let mut excluded_games: Vec<(String, String)> = Vec::new();

    for meta in &games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                excluded_games.push((meta.id.clone(), format!("no journal file: {e}")));
                continue;
            }
        };

        let mut game_actions: HashMap<ActionClass, u64> = HashMap::new();
        let mut turns: u64 = 0;
        let mut bad_card_in_game = false;

        for (lineno, line) in text.lines().enumerate() {
            if lineno == 0 {
                continue; // header
            }
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.splitn(5, '\t').collect();
            if fields.len() != 5 {
                continue;
            }
            if Color::parse(fields[1]).is_none() {
                continue; // malformed row: player_colour isn't one of the 4 known colours
            }
            let raw_text = fields[4];
            total_lines += 1;

            match classify(&card_index, raw_text) {
                LineOutcome::Action(c) => {
                    classified_lines += 1;
                    if c.class == ActionClass::EndTurn {
                        turns += 1;
                    }
                    *game_actions.entry(c.class).or_insert(0) += 1;
                    if card_expected(c.class) && c.card.is_none() {
                        bad_card_in_game = true;
                    }
                    if let Some(id) = c.card {
                        let name = id.get().base_name;
                        match c.class {
                            ActionClass::TakeCard => {
                                *takes_freq.entry(name).or_insert(0) += 1;
                            }
                            ActionClass::BuildBuilding
                            | ActionClass::BuildUnit
                            | ActionClass::BuildWonderStage
                            | ActionClass::DevelopTechnology
                            | ActionClass::ElectLeader
                            | ActionClass::ChangeGovernment
                            | ActionClass::PlayTactic
                            | ActionClass::DeclareWar
                            | ActionClass::PlayAggression
                            | ActionClass::ProposePact
                            | ActionClass::Colonize
                            | ActionClass::PlayActionCard => {
                                *plays_freq.entry(name).or_insert(0) += 1;
                            }
                            _ => {}
                        }
                    }
                }
                LineOutcome::Bookkeeping => {
                    classified_lines += 1;
                }
                LineOutcome::Unclassified => {
                    let shape = normalize_shape(raw_text);
                    *unclassified_shapes.entry(shape).or_insert(0) += 1;
                }
            }
        }

        if bad_card_in_game {
            excluded_games.push((
                meta.id.clone(),
                "card-carrying line matched a verb but no known base-game card name followed \
                 it (possible expansion card or unrecognised name)"
                    .to_string(),
            ));
            continue;
        }

        overall.add_game(meta, &game_actions, turns);
        by_players
            .entry(meta.players)
            .or_default()
            .add_game(meta, &game_actions, turns);
        by_tier
            .entry(meta.tier)
            .or_default()
            .add_game(meta, &game_actions, turns);
    }

    let included_games = games.len() - excluded_games.len();
    println!(
        "\nIncluded {included_games} of {} indexed games ({} excluded).",
        games.len(),
        excluded_games.len()
    );
    if !excluded_games.is_empty() {
        println!("\nExcluded games:");
        for (id, reason) in &excluded_games {
            println!("- {id}: {reason}");
        }
    }

    println!(
        "\nParser coverage: {classified_lines} / {total_lines} lines classified ({:.2}%).",
        classified_lines as f64 * 100.0 / total_lines.max(1) as f64
    );
    let mut shapes: Vec<(&String, &u64)> = unclassified_shapes.iter().collect();
    shapes.sort_by(|a, b| b.1.cmp(a.1));
    println!("\nTop unclassified shapes:");
    for (shape, count) in shapes.iter().take(20) {
        println!("- {count:>6}  {shape}");
    }

    println!("\n## Overall (all {included_games} games)");
    print_action_table(
        "Action rates, overall",
        &[("all", &overall)],
    );

    let p2 = by_players.get(&2).cloned_default();
    let p3 = by_players.get(&3).cloned_default();
    let p4 = by_players.get(&4).cloned_default();
    print_action_table(
        "Action rates by player count",
        &[("2p", &p2), ("3p", &p3), ("4p", &p4)],
    );

    let prince = by_tier.get(&Tier::Prince).cloned_default();
    let king = by_tier.get(&Tier::King).cloned_default();
    let warlord = by_tier.get(&Tier::Warlord).cloned_default();
    let emperor = by_tier.get(&Tier::Emperor).cloned_default();
    print_action_table(
        "Action rates by BGO level tier",
        &[
            (Tier::Prince.as_str(), &prince),
            (Tier::King.as_str(), &king),
            (Tier::Warlord.as_str(), &warlord),
            (Tier::Emperor.as_str(), &emperor),
        ],
    );

    println!("\n### Military summary, per game, by player count\n");
    println!("| | 2p (n={}) | 3p (n={}) | 4p (n={}) |", p2.games, p3.games, p4.games);
    println!("|---|---|---|---|");
    println!(
        "| games with >=1 war declared | {} ({:.1}%) | {} ({:.1}%) | {} ({:.1}%) |",
        p2.games_with_war,
        p2.games_with_war as f64 * 100.0 / p2.games.max(1) as f64,
        p3.games_with_war,
        p3.games_with_war as f64 * 100.0 / p3.games.max(1) as f64,
        p4.games_with_war,
        p4.games_with_war as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| games with >=1 aggression played | {} ({:.1}%) | {} ({:.1}%) | {} ({:.1}%) |",
        p2.games_with_aggression,
        p2.games_with_aggression as f64 * 100.0 / p2.games.max(1) as f64,
        p3.games_with_aggression,
        p3.games_with_aggression as f64 * 100.0 / p3.games.max(1) as f64,
        p4.games_with_aggression,
        p4.games_with_aggression as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| games with >=1 pact proposed | {} ({:.1}%) | {} ({:.1}%) | {} ({:.1}%) |",
        p2.games_with_pact,
        p2.games_with_pact as f64 * 100.0 / p2.games.max(1) as f64,
        p3.games_with_pact,
        p3.games_with_pact as f64 * 100.0 / p3.games.max(1) as f64,
        p4.games_with_pact,
        p4.games_with_pact as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| wars declared /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::DeclareWar),
        p3.per_game(ActionClass::DeclareWar),
        p4.per_game(ActionClass::DeclareWar)
    );
    println!(
        "| aggressions played /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::PlayAggression),
        p3.per_game(ActionClass::PlayAggression),
        p4.per_game(ActionClass::PlayAggression)
    );
    println!(
        "| tactics played /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::PlayTactic),
        p3.per_game(ActionClass::PlayTactic),
        p4.per_game(ActionClass::PlayTactic)
    );
    println!(
        "| pacts proposed /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::ProposePact),
        p3.per_game(ActionClass::ProposePact),
        p4.per_game(ActionClass::ProposePact)
    );
    println!(
        "| pacts accepted /game | {:.3} | {:.3} | {:.3} |",
        p2.per_game(ActionClass::AcceptPact),
        p3.per_game(ActionClass::AcceptPact),
        p4.per_game(ActionClass::AcceptPact)
    );

    println!("\n### Game length and scoring, by player count\n");
    println!("| | 2p | 3p | 4p |");
    println!("|---|---|---|---|");
    println!(
        "| mean rounds | {:.2} | {:.2} | {:.2} |",
        p2.rounds_sum as f64 / p2.games.max(1) as f64,
        p3.rounds_sum as f64 / p3.games.max(1) as f64,
        p4.rounds_sum as f64 / p4.games.max(1) as f64
    );
    println!(
        "| reached Age IV | {}/{} ({:.1}%) | {}/{} ({:.1}%) | {}/{} ({:.1}%) |",
        p2.age_iv_games, p2.games, p2.age_iv_games as f64 * 100.0 / p2.games.max(1) as f64,
        p3.age_iv_games, p3.games, p3.age_iv_games as f64 * 100.0 / p3.games.max(1) as f64,
        p4.age_iv_games, p4.games, p4.age_iv_games as f64 * 100.0 / p4.games.max(1) as f64
    );
    println!(
        "| mean final score | {:.1} | {:.1} | {:.1} |",
        p2.score_sum as f64 / p2.score_n.max(1) as f64,
        p3.score_sum as f64 / p3.score_n.max(1) as f64,
        p4.score_sum as f64 / p4.score_n.max(1) as f64
    );
    println!(
        "| max final score seen | {} | {} | {} |",
        p2.score_max, p3.score_max, p4.score_max
    );

    print_top_n("Cards taken from the row (frequency)", &takes_freq, 25);
    print_top_n("Cards played/built/discovered (frequency)", &plays_freq, 25);

    Ok(())
}

/// `HashMap::get(&K).cloned_default()`-style helper: an absent bucket (no
/// games at all in that split) should print as zeros, not panic or vanish
/// from the table.
trait ClonedDefault<T> {
    fn cloned_default(self) -> T;
}
impl ClonedDefault<Bucket> for Option<&Bucket> {
    fn cloned_default(self) -> Bucket {
        match self {
            Some(b) => Bucket {
                games: b.games,
                turns: b.turns,
                rounds_sum: b.rounds_sum,
                age_iv_games: b.age_iv_games,
                score_sum: b.score_sum,
                score_n: b.score_n,
                score_max: b.score_max,
                action_counts: b.action_counts.clone(),
                games_with_war: b.games_with_war,
                games_with_aggression: b.games_with_aggression,
                games_with_pact: b.games_with_pact,
                games_with_tactic: b.games_with_tactic,
            },
            None => Bucket::default(),
        }
    }
}

/// Normalises one unclassified line for shape histogramming: strips actor
/// colours and digit runs so that e.g. "Orange scores 4 culture" and
/// "Purple scores 11 culture" collapse to the same bucket in the coverage
/// report. Deliberately crude (no card-name stripping) since this only runs
/// on the residue this file failed to classify, and the report just needs
/// to name the shape, not fully parse it.
fn normalize_shape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c.is_ascii_digit() {
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                chars.next();
            }
            out.push('#');
        } else {
            out.push(c);
        }
    }
    for color in ["Orange", "Purple", "Green", "Grey"] {
        out = out.replace(color, "<C>");
    }
    out.chars().take(160).collect()
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() != 2 {
        eprintln!("usage: corpuscensus <index.tsv> <journals_dir>");
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

