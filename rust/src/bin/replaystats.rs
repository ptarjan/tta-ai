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
    let mut n_completed = 0u32;
    let mut round_reached_sum = 0u64;
    let mut rounds_total_sum = 0u64;
    let mut decisions_total = 0u64;
    let mut decisions_age_two_plus = 0u64;

    for meta in &games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let Ok(text) = fs::read_to_string(&path) else {
            continue; // no journal file for this id -- skip, don't count
        };
        n_games += 1;
        rounds_total_sum += meta.rounds as u64;

        let result = replay_game(meta, &text, &card_index, true);

        for d in &result.decisions {
            decisions_total += 1;
            if line_age(&text, d.lineno).is_some_and(is_age_two_plus) {
                decisions_age_two_plus += 1;
            }
        }

        if result.completed {
            n_completed += 1;
            round_reached_sum += meta.rounds as u64;
            continue;
        }
        let Some(m) = &result.mismatch else {
            continue; // stopped with no reason recorded -- shouldn't happen, don't crash reporting on it
        };
        let round_reached: u32 = m.round.parse().unwrap_or(0);
        round_reached_sum += round_reached as u64;
        let key = bucket_key(&m.kind);
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
    println!(
        "mean rounds reached: {:.2} (mean {:.2} total rounds per sampled game)",
        round_reached_sum as f64 / n_games.max(1) as f64,
        rounds_total_sum as f64 / n_games.max(1) as f64
    );
    println!(
        "decisions recorded: {decisions_total} ({:.1}% in Age II or later)\n",
        100.0 * decisions_age_two_plus as f64 / decisions_total.max(1) as f64
    );
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
