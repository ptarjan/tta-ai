//! `replaystats` -- a ranked histogram of WHY `replay.rs`'s reconstruction
//! (`tta::replay_common::replay_game`, see `docs/REPLAY.md`) stops before a
//! game's journal runs out, over a sample of the BGO human corpus, plus the
//! two summary numbers `docs/REPLAY.md`'s six prior passes have each
//! reported by hand: mean rounds reached (against each sampled game's own
//! `index.tsv`-recorded length) and the share of recorded decision points
//! that fall in Age II or later. Exists because every prior pass measured
//! its "what's blocking now" ranking by eyeballing raw per-game output --
//! this makes that measurement repeatable, and gives the ranking BEFORE
//! picking what to fix next, per this project's "measure first" rule.
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
//! cargo run --profile difftest --bin replaystats -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals [sample_size]
//! ```
//!
//! With no `sample_size`, replays every game in `index.tsv` that has a
//! journal file (1,011 as of this writing). `sample_size` takes the first N
//! games in `index.tsv` order -- no shuffling, matching every prior pass's
//! "no cherry-picking" sampling convention.
//!
//! Also prints a final-score cross-check: for every game whose replay both
//! reaches the journal's "End of game" marker AND actually flips
//! `state.game_over` (`GameResult::engine_scores`, `Some` only then -- see
//! `replay_common::replay_game`'s own doc on why those two conditions used to
//! diverge), this compares `game::scores` against `index.tsv`'s own recorded
//! result. Neither side is known to line engine seat `i` up with index
//! column `i` (`corpus::GameMeta::names` is index.tsv's own column order,
//! not seating order, and the journal never prints a player's real name --
//! see that field's own doc), so, like `bin/replay.rs`'s existing per-game
//! `match=` field, this compares the two SORTED score lists: an exact
//! multiset match is not fooled by a coincidental sum collision across two
//! different players, and for the common 2-player case a sorted pairing is
//! also a same-rank pairing whenever both scores are informative (matches
//! `bin/replay.rs`'s established comparison, not a new convention).

use std::collections::HashMap;
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus;
use tta::replay_common::{build_card_index, replay_game, MismatchKind};

/// A stable bucket key for one stop reason -- coarse enough to rank
/// meaningfully, fine enough that `IllegalMove` alone (structurally the
/// largest variant, since it covers every kind of rejected `Move`) doesn't
/// swallow every distinct symptom into one bucket.
fn bucket_key(kind: &MismatchKind) -> String {
    match kind {
        MismatchKind::UnrecoverableHiddenInfo(s) => format!("UnrecoverableHiddenInfo: {}", normalize(s)),
        MismatchKind::StuckPending(s) => format!("StuckPending: {}", normalize(s)),
        MismatchKind::ParserGap(s) => format!("ParserGap: {}", normalize(s)),
        MismatchKind::EventPlanInfeasible(s) => format!("EventPlanInfeasible: {}", normalize(s)),
        MismatchKind::IllegalMove { attempted, .. } => {
            format!("IllegalMove: {}", move_kind(attempted))
        }
    }
}

/// `Move`'s own variant name is the first identifier in its `{:?}`
/// rendering -- e.g. `"Take { slot: 2 }"` -> `"Take"`, `"WonderStep(3)"` ->
/// `"WonderStep"`. Cheap, and needs nothing beyond the `Debug` `Move`
/// already derives.
fn move_kind(attempted: &str) -> &str {
    attempted
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .find(|s| !s.is_empty())
        .unwrap_or(attempted)
}

/// Collapses digit runs to a single `#` so e.g. "outside 3p seating" and
/// "outside 4p seating" land in the same bucket. Most `MismatchKind`
/// messages are literal `&str`s with no interpolation at all -- this only
/// changes behaviour for the handful built with `format!`.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_digits = false;
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
                in_digits = true;
            }
        } else {
            in_digits = false;
            out.push(c);
        }
    }
    out
}

/// The journal's own per-line age column (`"A"`/`"I"`/`"II"`/`"III"`/
/// `"IV"`) for a 1-based file line number, matching `Decision::lineno` and
/// `Mismatch::lineno`'s numbering (`replay_common::parse_lines` sets
/// `lineno: i + 1` over `journal_text.lines()`'s 0-based enumeration).
/// Duplicated from that private function's own column split rather than
/// exposed from it -- five lines of `str::split`, and this is the only
/// other call site that needs it.
fn line_age(journal_text: &str, lineno: usize) -> Option<&str> {
    let line = journal_text.lines().nth(lineno.checked_sub(1)?)?;
    let fields: Vec<&str> = line.splitn(5, '\t').collect();
    (fields.len() == 5).then_some(fields[2])
}

fn is_age_two_plus(age: &str) -> bool {
    matches!(age, "II" | "III" | "IV")
}

struct Bucket {
    count: u32,
    round_sum: u32,
    example: String,
}

fn run(index_path: &str, journals_dir: &str, sample_size: Option<usize>) -> Result<(), String> {
    let card_index = build_card_index();
    let mut games = corpus::parse_index(index_path)?;
    if let Some(n) = sample_size {
        games.truncate(n);
    }

    let mut buckets: HashMap<String, Bucket> = HashMap::new();
    let mut n_games = 0u32;
    let mut bid_ceilings_grounded: u32 = 0;
    let mut n_bid_ceiling_games: u32 = 0;
    let mut hand_full_takes_overridden: u32 = 0;
    let mut n_hand_full_take_games: u32 = 0;
    let mut n_completed = 0u32;
    let mut round_reached_sum = 0u64;
    let mut rounds_total_sum = 0u64;
    let mut decisions_total = 0u64;
    let mut decisions_age_two_plus = 0u64;
    let mut n_score_checked = 0u32;
    let mut n_score_exact = 0u32;
    // One entry per non-matching player-score, `engine - index` after
    // sorting both lists ascending (see the module doc for why sorted, not
    // seated) -- a systematic non-zero skew here (rather than a spread
    // straddling zero) would name a scoring bug directly.
    let mut score_deltas: Vec<i32> = Vec::new();
    // `Replayer::check_discard_phase_oracle`'s own coverage stat -- see
    // `replay_common`'s "Discard-phase hand-size oracle" module doc and
    // `docs/REPLAY.md`'s "concrete, unused lead" section this implements.
    let mut discard_oracle_checked_total = 0u64;
    let mut discard_oracle_agreed_total = 0u64;
    // One formatted line per game whose reconstruction disagreed with the
    // journal's own cross-validated discard count -- FIRST divergence only,
    // per the same function's own doc.
    let mut discard_oracle_divergences: Vec<String> = Vec::new();
    // `GameResult::civil_deck_premature_advance` -- see that field's own
    // doc and `docs/REPLAY.md`'s "civil deck model" handoff. One example
    // per game, capped, so a full-corpus run doesn't dump hundreds of lines.
    let mut n_premature: u32 = 0;
    let mut premature_examples: Vec<String> = Vec::new();

    for meta in &games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let Ok(text) = fs::read_to_string(&path) else {
            continue; // no journal file for this id -- skip, don't count
        };
        n_games += 1;
        rounds_total_sum += meta.rounds as u64;

        if std::env::var("REPLAY_DEBUG").is_ok() {
            eprintln!("DEBUG game={}", meta.id);
        }
        let result = replay_game(meta, &text, &card_index, true);
        bid_ceilings_grounded += result.bid_ceilings_grounded;
        if result.bid_ceilings_grounded > 0 {
            n_bid_ceiling_games += 1;
        }
        hand_full_takes_overridden += result.hand_full_takes_overridden;
        if result.hand_full_takes_overridden > 0 {
            n_hand_full_take_games += 1;
        }
        discard_oracle_checked_total += result.discard_oracle_checked as u64;
        discard_oracle_agreed_total += result.discard_oracle_agreed as u64;
        if let Some(d) = &result.discard_oracle_divergence {
            discard_oracle_divergences.push(format!(
                "{} line {} (round {} age {}, {}): journal's cross-validated excess {}, this binary computes {} \
                 (hand_military_len {} limit {})",
                meta.id, d.lineno, d.round, d.age, d.actor, d.journal_excess, d.reconstructed_excess, d.hand_len, d.limit
            ));
        }
        if let Some(p) = &result.civil_deck_premature_advance {
            n_premature += 1;
            if premature_examples.len() < 10 {
                premature_examples.push(format!(
                    "{} line {}: reconstructed age {:?} ahead of journal's own {:?}",
                    meta.id, p.lineno, p.reconstructed_age, p.journal_age
                ));
            }
        }

        for d in &result.decisions {
            decisions_total += 1;
            if line_age(&text, d.lineno).is_some_and(is_age_two_plus) {
                decisions_age_two_plus += 1;
            }
        }

        if result.completed {
            n_completed += 1;
            round_reached_sum += meta.rounds as u64;
            if std::env::var("REPLAY_DEBUG").is_ok() {
                eprintln!("DEBUG completed: {}", meta.id);
            }
            if let Some(engine) = &result.engine_scores {
                n_score_checked += 1;
                let mut a = engine.clone();
                let mut b = result.index_scores.clone();
                a.sort_unstable();
                b.sort_unstable();
                if a == b {
                    n_score_exact += 1;
                } else {
                    score_deltas.extend(a.iter().zip(b.iter()).map(|(x, y)| x - y));
                }
            }
            continue;
        }
        let Some(m) = &result.mismatch else {
            continue; // stopped with no reason recorded -- shouldn't happen, don't crash reporting on it
        };
        let round_reached: u32 = m.round.parse().unwrap_or(0);
        round_reached_sum += round_reached as u64;
        let key = bucket_key(&m.kind);
        if std::env::var("REPLAY_DUMP_BUCKET").is_ok_and(|want| key.contains(&want)) {
            eprintln!("DUMP {} line {}: {}", meta.id, m.lineno, m.raw_text);
        }
        let b = buckets.entry(key).or_insert_with(|| Bucket {
            count: 0,
            round_sum: 0,
            example: format!("{} line {}: {}", meta.id, m.lineno, m.raw_text),
        });
        b.count += 1;
        b.round_sum += round_reached;
    }

    let mut ranked: Vec<(&String, &Bucket)> = buckets.iter().collect();
    ranked.sort_unstable_by(|a, b| b.1.count.cmp(&a.1.count));

    println!("# replaystats: {n_games} games sampled, {n_completed} completed to state.game_over\n");
    println!("## Final-score cross-check\n");
    println!(
        "{n_score_checked}/{n_completed} completed games actually flipped state.game_over (the other \
         {} reached the journal's own \"End of game\" marker but hit a mismatch afterward, so \
         `game::scores` was never computed for them); {n_score_exact}/{n_score_checked} of those matched \
         index.tsv exactly (sorted per-game score-list compare).",
        n_completed.saturating_sub(n_score_checked)
    );
    if score_deltas.is_empty() {
        if n_score_checked > 0 {
            println!("No non-matching player-scores to report a delta distribution over.");
        }
    } else {
        score_deltas.sort_unstable();
        let sum: i64 = score_deltas.iter().map(|&d| d as i64).sum();
        let mean = sum as f64 / score_deltas.len() as f64;
        println!(
            "delta distribution for the {} non-matching player-scores (engine minus index.tsv, sorted \
             ascending, mean {mean:.2}): {score_deltas:?}\n",
            score_deltas.len()
        );
    }
    println!(
        "mean rounds reached: {:.2} (mean {:.2} total rounds per sampled game)",
        round_reached_sum as f64 / n_games.max(1) as f64,
        rounds_total_sum as f64 / n_games.max(1) as f64
    );
    println!(
        "decisions recorded: {decisions_total} ({:.1}% in Age II or later)",
        100.0 * decisions_age_two_plus as f64 / decisions_total.max(1) as f64
    );
    // Reported, never folded into the numbers above: each of these is a
    // hand slot whose identity was deduced from a logged bid rather than
    // read off a line naming the card -- see
    // `replay_common::Replayer::ground_bid_ceiling`.
    println!("hand cards grounded from a bid's own force ceiling: {bid_ceilings_grounded} (in {n_bid_ceiling_games} games)");
    // `GameResult::hand_full_takes_overridden` -- `docs/REPLAY.md`'s
    // Take/HandFull "genuinely unexplained discrepancy" conclusion: a
    // deliberate REPLAYER-ONLY divergence from self-play legality
    // (`costs::take_gate`'s `hand_full` gate stays rulebook-correct and
    // untouched). Reported so this is never quietly papered over -- the
    // known corpus shape is ~109 games' worth; a count far above that here
    // is a signal the override (`take_blocked_only_by_hand_full`) is too
    // loose, not confirmation it is working.
    println!(
        "journal-observed takes accepted despite failing ONLY the hand_full gate: {hand_full_takes_overridden} \
         (in {n_hand_full_take_games} games; the known corpus shape is ~109 games' worth -- a much larger number \
         here means the override is too loose)\n"
    );

    println!("## Discard-phase hand-size oracle\n");
    println!(
        "{discard_oracle_checked_total} `(actor, round)` checkpoints had a cross-validated journal count to check \
         this binary's own reconstructed military-hand excess against (see `replay_common`'s \"Discard-phase \
         hand-size oracle\" module doc and `docs/REPLAY.md`); {discard_oracle_agreed_total} ({:.1}%) matched exactly.",
        100.0 * discard_oracle_agreed_total as f64 / discard_oracle_checked_total.max(1) as f64
    );
    println!(
        "{}/{n_games} games sampled had at least one checkpoint disagree (FIRST divergence only, one line per \
         game -- walk each back from here to find where the drift actually starts):\n",
        discard_oracle_divergences.len()
    );
    for line in &discard_oracle_divergences {
        println!("- {line}");
    }
    println!();

    // `GameResult::civil_deck_premature_advance` -- docs/REPLAY.md's "civil
    // deck model" handoff. Zero here is the invariant `top_up_civil_deck`
    // is meant to guarantee; a nonzero count is this instrument catching a
    // regression, not a new investigation needed from scratch.
    println!("civil-age premature advances (this reconstruction's own age_civil read ahead of the journal's Line::age column, with more of the OLD age still to come): {n_premature} games");
    for ex in &premature_examples {
        println!("  {ex}");
    }
    println!();
    println!("## Stop-reason histogram, ranked by count\n");
    println!("| count | mean round reached | reason | example |");
    println!("|---|---|---|---|");
    for (key, b) in ranked {
        println!(
            "| {} | {:.1} | {} | {} |",
            b.count,
            b.round_sum as f64 / b.count as f64,
            key,
            b.example.replace('|', "\\|")
        );
    }

    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 2 {
        eprintln!("usage: replaystats <index.tsv> <journals_dir> [sample_size]");
        return ExitCode::FAILURE;
    }
    let sample_size = argv.get(2).and_then(|s| s.parse().ok());
    match run(&argv[0], &argv[1], sample_size) {
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

    #[test]
    fn move_kind_extracts_the_variant_name_from_a_struct_style_debug_string() {
        assert_eq!(move_kind("Take { slot: 2 }"), "Take");
    }

    #[test]
    fn move_kind_extracts_the_variant_name_from_a_tuple_style_debug_string() {
        assert_eq!(move_kind("WonderStep(3)"), "WonderStep");
    }

    #[test]
    fn move_kind_extracts_the_variant_name_from_a_unit_style_debug_string() {
        assert_eq!(move_kind("EndTurn"), "EndTurn");
    }

    #[test]
    fn normalize_collapses_a_run_of_digits_to_one_hash() {
        assert_eq!(normalize("outside 12p seating"), "outside #p seating");
    }

    #[test]
    fn normalize_collapses_two_separate_digit_runs_independently() {
        assert_eq!(normalize("line 5 round 19"), "line # round #");
    }

    #[test]
    fn normalize_is_unchanged_for_a_message_with_no_digits() {
        assert_eq!(
            normalize("resolve_intervening did not converge"),
            "resolve_intervening did not converge"
        );
    }

    #[test]
    fn line_age_reads_the_third_tab_separated_column_at_a_one_based_line_number() {
        let journal = "date\tplayer_colour\tage\tround\ttext\n2026-01-01\tOrange\tII\t8\tOrange builds Foo\n";
        assert_eq!(line_age(journal, 2), Some("II"));
    }

    #[test]
    fn line_age_returns_none_past_the_end_of_the_journal() {
        let journal = "header\n2026-01-01\tOrange\tA\t1\ttext\n";
        assert_eq!(line_age(journal, 99), None);
    }

    #[test]
    fn is_age_two_plus_is_true_for_ii_iii_and_iv_and_false_for_a_and_i() {
        assert!(is_age_two_plus("II"));
        assert!(is_age_two_plus("III"));
        assert!(is_age_two_plus("IV"));
        assert!(!is_age_two_plus("A"));
        assert!(!is_age_two_plus("I"));
    }

    #[test]
    fn bucket_key_groups_illegal_move_by_its_move_variant_not_its_full_debug_string() {
        let a = MismatchKind::IllegalMove {
            attempted: "Take { slot: 1 }".into(),
            legal_moves: "[]".into(),
        };
        let b = MismatchKind::IllegalMove {
            attempted: "Take { slot: 4 }".into(),
            legal_moves: "[EndTurn]".into(),
        };
        assert_eq!(bucket_key(&a), bucket_key(&b));
    }
}
