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
use tta::corpus::ActionClass;
use tta::replay_common::{
    build_card_index, replay_game, CultureOracleDivergence, FinalEventAwardDivergence, HandLedgerVerdict,
    LedgerEventKind, MismatchKind,
};

/// A stable, printable bucket key for [`HandLedgerVerdict`] -- collapses the
/// carried/looked-up [`LedgerEventKind`] to its `Debug` name so the
/// classification histogram groups by MECHANISM, not by the `Option` wrapper
/// around it. `last_event` is passed separately (rather than read off
/// `HandLedgerVerdict::UnmodelledEvent` alone) so `SimulatorBug` -- whose
/// variant carries no event of its own -- can ALSO be sub-bucketed by what
/// ledger event most recently preceded it: even though the ledger agrees
/// with the journal there (so no event class is missing), knowing which
/// mechanism was active right before the simulator's own state went wrong is
/// exactly the clue needed to find the specific buggy call site.
fn hand_ledger_verdict_key(v: &HandLedgerVerdict, last_event: Option<LedgerEventKind>) -> String {
    match v {
        HandLedgerVerdict::SimulatorBug => format!(
            "SimulatorBug (ledger agrees with journal; forward simulator's own hand_military is wrong) -- \
             last ledger event was {}",
            last_event.map(|k| format!("{k:?}")).unwrap_or_else(|| "none".to_string())
        ),
        HandLedgerVerdict::UnmodelledEvent(Some(kind)) => format!("UnmodelledEvent: last ledger event was {kind:?}"),
        HandLedgerVerdict::UnmodelledEvent(None) => "UnmodelledEvent: no prior ledger event at all".to_string(),
        HandLedgerVerdict::NoLedgerEntry => "NoLedgerEntry (ledger coverage gap)".to_string(),
    }
}

/// The culture-oracle cause histogram's own bucket key: the `Debug` name of
/// whatever [`ActionClass`] was the last classified action line strictly
/// before this game's first culture-oracle divergence, or a named bucket for
/// "no prior classified action at all" (a divergence on the very first "End
/// turn" of the game). Bucket names describe the SYMPTOM location (what
/// happened right before the checkpoint noticed a drift), NOT necessarily
/// the cause -- see `docs/REPLAY.md`'s "Culture-oracle" section for why the
/// true first divergence is routinely several rounds earlier than this.
fn action_class_bucket_key(last_action_class: Option<ActionClass>) -> String {
    match last_action_class {
        Some(class) => format!("{class:?}"),
        None => "(no prior classified action this game)".to_string(),
    }
}

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

/// The JOURNAL-ARITHMETIC-ERROR GATE's own predicate: `Some((seat, d))`
/// when this game's final-score divergence is EXACTLY ONE seat's score
/// being off by EXACTLY one final-award pair's `(engine - journal)` delta,
/// and the game's own in-play culture checkpoints matched the engine to the
/// point (no `culture_oracle_divergence`) -- i.e. the engine's
/// end-of-game recompute is the formula's ground truth, the journal's
/// stated amount for that (card, seat) is BGO's own record error (a stale
/// in-play magnitude, a mis-announced number, or a "scores N" where the
/// true clause is "loses N"), and nothing ELSE in the game disagrees.
/// `None` otherwise: multiple seats off (the deltas might cancel in the
/// sorted compare and still hide two real engine bugs), no award pair
/// matching the seat's delta (the divergence is not a final-award error at
/// all), a DIRECTION FLIP (journal and engine disagree on SIGN, not just
/// magnitude -- a genuine `scoring_culture` formula bug that must surface),
/// or any in-play culture drift at all (the engine's own running totals
/// disagreed with the journal somewhere, so "the engine is right" is not
/// proven for this game).
///
/// `a` is the engine's own `game::scores` in SEATING order (Orange=0,
/// Purple=1, Green=2, Grey=3). `b` is the journal's index.tsv totals --
/// but index.tsv's `results` column is in PLACEMENT order (winner first),
/// NOT seat order, so `b` cannot be compared against `a` position-by-position
/// on its own: `index_order` (see `corpus::GameMeta::index_order`) maps each
/// index position to its seat, and this gate seats `b` via that mapping
/// before comparing. `index_order` is `None` when the journal's winner
/// could not be pinned (no usable "End turn ... scores:" line), in which
/// case the gate cannot seat `b` and conservatively returns `None`. The
/// caller only calls this in the sorted-compare `a != b` case.
/// First corpus case: game 7521849's "Impact of Progress" (journal's
/// "Purple scores 12" is a stale in-play magnitude; the true end-board
/// scores 14 under the card's own wording -- see
/// `events::scoring_culture`'s own doc). The `scorediv_gate` test module
/// below pins every branch of this predicate against fixtures.
fn journal_arithmetic_error_suppression(
    a: &[i32],
    b: &[i32],
    index_order: Option<&[u8]>,
    divergences: &[FinalEventAwardDivergence],
    culture_oracle_divergence: Option<&CultureOracleDivergence>,
) -> Option<(String, FinalEventAwardDivergence)> {
    if culture_oracle_divergence.is_some() {
        return None;
    }
    let mut candidates: Vec<(&FinalEventAwardDivergence, i32)> = Vec::new();
    for d in divergences {
        let delta = d.engine_amount - d.journal_amount;
        // A direction flip (journal and engine disagree on SIGN, not just
        // magnitude) is a genuine `scoring_culture` formula bug, never
        // suppressed.
        if (d.journal_amount >= 0 && d.engine_amount < 0) || (d.journal_amount < 0 && d.engine_amount >= 0) {
            continue;
        }
        candidates.push((d, delta));
    }
    // Exactly one (card, seat) pair may explain the whole game.
    if candidates.len() != 1 {
        return None;
    }
    let (d, delta) = candidates.pop().unwrap();
    if a.len() != b.len() || delta == 0 {
        return None;
    }
    // Seat `b` (index.tsv's PLACEMENT-order scores) into seating order via
    // `index_order`: `index_order[pos]` is the seat that occupies index
    // position `pos`. `a` is already in seating order, so a per-seat diff
    // `a[seat] - b_seated[seat]` is now meaningful. Without a pinned
    // winner (`index_order` None) we cannot seat `b` and must not guess.
    let Some(index_order) = index_order else {
        return None;
    };
    if index_order.len() != b.len() {
        return None;
    }
    let mut b_seated = vec![i32::MIN; b.len()];
    for (pos, &seat) in index_order.iter().enumerate() {
        if seat as usize >= b_seated.len() {
            return None;
        }
        b_seated[seat as usize] = b[pos];
    }
    // The pair's own seat must be the ONLY seat whose score differs, and
    // by exactly this pair's delta.
    let mut non_zero: Vec<(usize, i32)> = Vec::new();
    for (seat, (x, y)) in a.iter().zip(b_seated.iter()).enumerate() {
        let diff = x - y;
        if diff != 0 {
            non_zero.push((seat, diff));
        }
    }
    if non_zero.len() != 1 {
        return None;
    }
    let (seat, diff) = non_zero[0];
    if diff != delta {
        return None;
    }
    if seat as u8 != d.seat {
        return None;
    }
    let seat_name = crate::corpus::Color::from_seat(d.seat)?.as_str().to_string();
    Some((seat_name, d.clone()))
}
fn run(index_path: &str, journals_dir: &str, sample_size: Option<usize>, only_game_ids: Option<&[String]>) -> Result<(), String> {
    let card_index = build_card_index();
    let mut games = corpus::parse_index(index_path)?;
    if let Some(ids) = only_game_ids {
        games.retain(|g| ids.contains(&g.id));
    } else if let Some(n) = sample_size {
        games.truncate(n);
    }
    // Seat the index.tsv `results` column: it is in PLACEMENT order (winner
    // first), not seat order, and the journal-arithmetic-error gate below
    // needs to know WHICH seat each index position maps to. Derive that from
    // each game's own journal (winner's colour + stated total pin the
    // winner's seat; the rest follow by score). Missing journal -> `None`,
    // and the gate conservatively declines that game.
    corpus::fill_index_order(&mut games, journals_dir);

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
    // JOURNAL-ARITHMETIC-ERROR GATE counter: games whose ONLY final-score
    // divergence is a final-award pair the gate itself re-derives as BGO's
    // own record error (see `journal_arithmetic_error_suppression`) --
    // reported after `n_score_exact` so the two add up to the full
    // non-mismatch set.
    let mut n_gate_suppressed = 0u32;
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
    // `GameResult::hand_ledger_verdict`'s corpus-wide histogram -- the
    // deliverable this binary exists to compute: classifies WHY each
    // game's first discard-phase-oracle divergence happened, not just THAT
    // it did. One example repro line kept per bucket (the same
    // `discard_oracle_divergences`-style formatting, first one seen wins).
    let mut ledger_verdict_buckets: HashMap<String, (u32, String)> = HashMap::new();
    // `GameResult::civil_deck_premature_advance` -- see that field's own
    // doc and `docs/REPLAY.md`'s "civil deck model" handoff. One example
    // per game, capped, so a full-corpus run doesn't dump hundreds of lines.
    let mut n_premature: u32 = 0;
    let mut premature_examples: Vec<String> = Vec::new();
    // `GameResult::politics_false_skips` -- see that field's own doc and
    // `docs/REPLAY.md`'s "Final scores" section: a false skip is the
    // mechanism traced from the final-score cross-check above, named
    // structurally rather than re-derived per game.
    let mut n_false_skips_total: u64 = 0;
    let mut n_games_with_false_skip: u32 = 0;
    // `GameResult::culture_oracle_checked`/`culture_oracle_agreed` -- see
    // `replay_common`'s `CultureOracleDivergence` doc and `docs/REPLAY.md`'s
    // "Culture-oracle cause-classification instrument" section this
    // implements: culture IS the score in this game, so this coverage stat
    // matters as much as the discard-phase oracle's own.
    let mut culture_oracle_checked_total = 0u64;
    let mut culture_oracle_agreed_total = 0u64;
    // `GameResult::science_oracle_checked`/`_agreed`'s twin -- SCIDRIFT
    // 2026-08-14 pass, `SCIENCE_ORACLE`-gated (see `debugflags::
    // science_oracle`'s own doc): both stay 0 unless that env var is set for
    // this run, guaranteeing a plain `replaystats` invocation is unaffected.
    let mut science_oracle_checked_total = 0u64;
    let mut science_oracle_agreed_total = 0u64;
    let mut n_science_diverging_games = 0u32;
    // `GameResult::resource_oracle_checked`/`_agreed`'s twin, `RESOURCE_ORACLE`-gated.
    let mut resource_oracle_checked_total = 0u64;
    let mut resource_oracle_agreed_total = 0u64;
    let mut n_resource_diverging_games = 0u32;
    // The task's own deliverable: FIRST-divergence-per-game, ranked by the
    // `ActionClass` of whatever the last classified action line was
    // strictly before the diverging checkpoint. `None` keys (no prior
    // classified action at all this game, i.e. divergence on the very first
    // "End turn") bucket separately rather than being dropped.
    let mut culture_cause_buckets: HashMap<String, (u32, String)> = HashMap::new();
    // `GameResult::politics_false_skips_unrecovered` -- the TRUE damage
    // signal (that field's own doc, and `politics_false_skips`'s, explain
    // why the two are not the same number on purpose). Should read 0.
    let mut n_false_skips_unrecovered_total: u64 = 0;
    let mut n_games_with_unrecovered_false_skip: u32 = 0;
    // `GameResult::final_event_award_divergences`'s own corpus-wide ranking:
    // per-card (count of diverging (game, seat) pairs, one example line),
    // ranked so the single highest-impact `scoring_culture` formula bug
    // (§12.5.2) reads first -- the "which final-scoring CARD is wrong"
    // deliverable `docs/REPLAY.md`'s "Final scores" section still needed.
    let mut final_event_award_buckets: HashMap<&'static str, (u32, String)> = HashMap::new();
    let mut n_games_with_final_event_award_divergence: u32 = 0;

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
            if let Some(verdict) = &result.hand_ledger_verdict {
                let key = hand_ledger_verdict_key(verdict, d.ledger_last_event.map(|(kind, _)| kind));
                let example = format!(
                    "{} line {} (round {} age {}, {}): journal excess {}, simulator excess {}, ledger excess {} \
                     (ledger's own last event: {:?})",
                    meta.id,
                    d.lineno,
                    d.round,
                    d.age,
                    d.actor,
                    d.journal_excess,
                    d.reconstructed_excess,
                    d.ledger_excess,
                    d.ledger_last_event
                );
                if std::env::var("REPLAY_DUMP_BUCKET").is_ok_and(|want| key.contains(&want)) {
                    eprintln!("DUMP {example}");
                }
                let entry = ledger_verdict_buckets.entry(key).or_insert_with(|| (0, example.clone()));
                entry.0 += 1;
            }
        }
        culture_oracle_checked_total += result.culture_oracle_checked as u64;
        culture_oracle_agreed_total += result.culture_oracle_agreed as u64;
        if let Some(d) = &result.culture_oracle_divergence {
            let key = action_class_bucket_key(d.last_action_class);
            let example = format!(
                "{} line {} ({}): journal says (now {}), this binary computes {} (delta {})",
                meta.id,
                d.lineno,
                d.actor,
                d.journal_now,
                d.reconstructed,
                d.reconstructed - d.journal_now
            );
            if std::env::var("REPLAY_DUMP_BUCKET").is_ok_and(|want| key.contains(&want)) {
                eprintln!("DUMP {example}");
            }
            let entry = culture_cause_buckets.entry(key).or_insert_with(|| (0, example.clone()));
            entry.0 += 1;
        }
        // Science/resource oracle twins of the culture block just above --
        // SCIDRIFT 2026-08-14 pass. Printed unconditionally whenever `Some`
        // (which itself only happens when `SCIENCE_ORACLE`/`RESOURCE_ORACLE`
        // was set for this run -- the caller already opted into the
        // diagnostic, unlike `SCOREDIV_DUMP_IDS`, which gates an
        // always-computed culture check that would otherwise flood every
        // ordinary run).
        science_oracle_checked_total += result.science_oracle_checked as u64;
        science_oracle_agreed_total += result.science_oracle_agreed as u64;
        if let Some(d) = &result.science_oracle_divergence {
            n_science_diverging_games += 1;
            println!("SCIENCE_DIVERGING_ID {}", meta.id);
            println!(
                "SCIENCE_DETAIL {} lineno={} round={} actor={} last_action_class={:?} journal_now={} \
                 reconstructed={} delta={}",
                meta.id,
                d.lineno,
                d.round,
                d.actor,
                d.last_action_class,
                d.journal_now,
                d.reconstructed,
                d.reconstructed - d.journal_now
            );
        }
        resource_oracle_checked_total += result.resource_oracle_checked as u64;
        resource_oracle_agreed_total += result.resource_oracle_agreed as u64;
        if let Some(d) = &result.resource_oracle_divergence {
            n_resource_diverging_games += 1;
            println!("RESOURCE_DIVERGING_ID {}", meta.id);
            println!(
                "RESOURCE_DETAIL {} lineno={} round={} actor={} last_action_class={:?} journal_now={} \
                 reconstructed={} delta={}",
                meta.id,
                d.lineno,
                d.round,
                d.actor,
                d.last_action_class,
                d.journal_now,
                d.reconstructed,
                d.reconstructed - d.journal_now
            );
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
        if result.politics_false_skips > 0 {
            n_false_skips_total += result.politics_false_skips as u64;
            n_games_with_false_skip += 1;
        }
        if result.politics_false_skips_unrecovered > 0 {
            n_false_skips_unrecovered_total += result.politics_false_skips_unrecovered as u64;
            n_games_with_unrecovered_false_skip += 1;
        }
        if !result.final_event_award_divergences.is_empty() {
            n_games_with_final_event_award_divergence += 1;
            for d in &result.final_event_award_divergences {
                let example = format!(
                    "{} seat {}: journal says {}, this binary computes {} (delta {})",
                    meta.id,
                    d.seat,
                    d.journal_amount,
                    d.engine_amount,
                    d.engine_amount - d.journal_amount
                );
                if std::env::var("REPLAY_DUMP_BUCKET").is_ok_and(|want| d.card.contains(&want)) {
                    eprintln!("DUMP {example}");
                }
                let entry = final_event_award_buckets.entry(d.card).or_insert_with(|| (0, example.clone()));
                entry.0 += 1;
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
            // A dedicated gate for JUST the completed-game IDs. `REPLAY_DEBUG`
            // above also turns on the engine's whole per-action trace, which is
            // far too much output to sit through merely to learn which games
            // finished. This list is the input to the replay-completion
            // regression guard: a fix may only be landed if every ID printed
            // here before the change is still printed after it. `replay`'s own
            // per-game COMPLETE/STOPPED verdict is NOT a substitute -- the two
            // binaries do not resolve hidden information the same way and
            // disagree on ~68 games, so a guard list built from `replay` silently
            // under-protects the games only `replaystats` finishes.
            if std::env::var("SCOREDIV_DUMP_COMPLETED").is_ok() {
                println!("SCOREDIV_COMPLETED_ID {}", meta.id);
            }
            if let Some(engine) = &result.engine_scores {
                n_score_checked += 1;
                let mut a = engine.clone();
                let mut b = result.index_scores.clone();
                // Per-seat (unsorted) copies for the journal-arithmetic-error
                // gate: `a` and `b` are sorted for the cross-check below, but
                // the gate needs to know WHICH seat's score differs, which
                // requires the original seating order.
                let a_seated = engine.clone();
                let b_seated = result.index_scores.clone();
                a.sort_unstable();
                b.sort_unstable();
                if a == b {
                    n_score_exact += 1;
                } else if let Some((seat, d)) = journal_arithmetic_error_suppression(
                    &a_seated,
                    &b_seated,
                    meta.index_order.as_deref(),
                    &result.final_event_award_divergences,
                    result.culture_oracle_divergence.as_ref(),
                )
                {
                    // JOURNAL-ARITHMETIC-ERROR GATE (2026-08-16, closing
                    // 7521849's false positive): this game's TOTAL matches
                    // index.tsv except for exactly one seat, and that
                    // seat's whole delta is explained by ONE final-award
                    // (card, seat) pair whose journal-stated amount is BGO's
                    // own record error (a stale in-play magnitude, a
                    // mis-announced number) -- the engine's own
                    // end-of-game recompute is the formula's ground truth,
                    // and the game's own culture checkpoints match the
                    // engine to the point, so the engine is right and the
                    // journal line is wrong. The game is EXCLUDED from the
                    // SCOREDIV_DIVERGING_ID / SCOREDIV_DETAIL lines below --
                    // the per-card `final_event_award_buckets` and
                    // `n_games_with_final_event_award_divergence` still
                    // count every (game, card, seat) pair whose amount
                    // differs (the corpus-wide "which card is wrong"
                    // ranking is unchanged), and the SCOREDIV_SUPPRESSED
                    // line below reports the game once with the exact
                    // tuple, so the suppression is visible and auditable
                    // rather than silent. 7521849's "Impact of Progress"
                    // (journal's "Purple scores 12" is a stale in-play
                    // magnitude; the true end-board scores 14 under the
                    // card's own wording) is the first corpus case; the
                    // gate's own test pins the shape against a fixture
                    // (the `scorediv_gate` module in this file).
                    n_gate_suppressed += 1;
                    if std::env::var("SCOREDIV_DUMP_IDS").is_ok() {
                        eprintln!(
                            "SCOREDIV_SUPPRESSED {} -- journal-arithmetic-error final-award divergence: {} seat{} \
                             journal={} engine={} (delta {}); engine total {a:?} vs index {b:?}",
                            meta.id, seat, d.seat, d.journal_amount, d.engine_amount,
                            d.engine_amount - d.journal_amount,
                        );
                    }
                } else {
                    score_deltas.extend(a.iter().zip(b.iter()).map(|(x, y)| x - y));
                    // `experiments/measure_replaystats.sh`'s own diverging-ID
                    // artifact and this file's cause-ranking investigation
                    // both read this line -- gated behind an env var (not
                    // printed by default) so a plain `replaystats` run's
                    // output stays exactly what it always was. Printed to
                    // stdout as a single grep-able prefix per game, plus the
                    // diagnostics needed to tell an accumulated-culture bug
                    // (this game's OWN culture_oracle_divergence is Some)
                    // apart from an end-of-game-only scoring bug (every "End
                    // turn" in this game matched, so the drift is entirely
                    // in `events::evaluate_final_events` /
                    // `game::end_of_game_bonus`, which run strictly after the
                    // last checkpoint the oracle above ever sees).
                    if std::env::var("SCOREDIV_DUMP_IDS").is_ok() {
                        println!("SCOREDIV_DIVERGING_ID {}", meta.id);
                        // 7523159 was traced by hand (2026-08-16,
                        // `analysis/worker_notes_2026-08-16/scorediv_scan_
                        // _2026-08-16_gate_fix.txt`): its journal's own
                        // "First Space Flight ... Purple scores 27" line is
                        // 1-3 off from BOTH the journal's own final board
                        // (techs summing 30) and the engine's reconstruction
                        // (techs 27 + gov 2 = 29) -- a stale/rounded
                        // mid-board value in BGO's own record, NOT a
                        // formula or engine bug. The formula (techs + gov)
                        // was tested by removing `+ government.level()` and
                        // broke 346 games; it stands. The note is pinned
                        // to this ID so a future sweep pass does not
                        // re-attempt the formula from this game's line.
                        let note = if meta.id == "7523159" {
                            " note=journal-internally-inconsistent(First Space Flight line \
                             scores 27 vs its own final board techs 30; formula stands -- \
                             see worker_notes_2026-08-16/scorediv_scan_2026-08-16_gate_fix.txt)"
                        } else if meta.id == "7522239" {
                            // 7522239 (2p, BGO record error): the journal's own
                            // running culture totals (Orange ends "now 42",
                            // Purple ends "now 36" at the last "End turn")
                            // contradict its own final "WINNER IS ... AS ORANGE
                            // (54 PTS); 2nd ... AS PURPLE (11 PTS)" line by 12 /
                            // 25 points respectively -- no end-of-game bonus set
                            // bridges that gap, so BGO's own record is
                            // self-inconsistent (the round-15 "concedes defeat"
                            // is an Armed Intervention refusal artifact, not a
                            // resignation -- Purple kept playing 5 more rounds).
                            // The index row [54, 11] copies the WINNER line; the
                            // engine's [14, 54] reconstruction matches neither.
                            // The gate cannot fire (zero mismatching final-award
                            // pairs -- every "Impact of ..." line agrees), so
                            // this is pinned here so a future sweep pass does
                            // not re-attempt a formula from this broken journal.
                            " note=journal-internally-inconsistent(running \
                             totals Orange 42 / Purple 36 vs its own WINNER line \
                             Orange 54 / Purple 11; no bonus set bridges the gap -- \
                             BGO record error, not an engine/formula bug)"
                        } else if meta.id == "7522397" {
                            // 7522397 (2p, BGO record error): the engine's own
                            // running culture matches BGO's "(now M)" at all 36
                            // "End turn" checkpoints (culture oracle 36/36), so
                            // the in-play reconstruction is correct. But the
                            // journal is internally inconsistent: BGO's last
                            // "End turn Orange" running total is "now 79", and
                            // the journal's OWN stated final awards for Orange
                            // (Strength 0, Population 2, Competition 8,
                            // Happiness 6) sum to 16 -- so BGO's own numbers
                            // give Orange 95, yet the index row says 110. The
                            // engine (116) is off from the index by +6, of
                            // which +4 is the single mismatching final-award
                            // pair (Impact of Happiness seat0: journal 6 vs
                            // engine 10 -- the engine counts Orange's 5 happy
                            // faces: Leonardo Da Vinci + 3 completed wonders +
                            // 1, all real on the board) and +2 is the
                            // unbridgeable index-vs-own-totals gap. No engine
                            // formula change reconciles a journal that
                            // contradicts itself, so this is pinned as a BGO
                            // record error rather than re-attempted.
                            " note=journal-internally-inconsistent(last \
                             'End turn Orange' running total 79 + its own stated \
                             final awards 16 = 95, but index row is 110; engine \
                             +6 is the Happiness seat0 pair (j6/e10, 5 real \
                             happy faces) plus the index-vs-own-totals gap -- \
                             BGO record error, not an engine/formula bug)"
                        } else {
                            ""
                        };
                        println!(
                            "SCOREDIV_DETAIL {} engine={:?} index={:?} culture_drifted_in_play={} \
                             final_event_cards={:?} first_culture_divergence={:?}{note}",
                            meta.id,
                            a,
                            b,
                            result.culture_oracle_divergence.is_some(),
                            result.final_event_cards,
                            result.culture_oracle_divergence.as_ref().map(|d| format!(
                                "lineno={} actor={} last_action_class={:?} delta={}",
                                d.lineno,
                                d.actor,
                                d.last_action_class,
                                d.reconstructed - d.journal_now
                            ))
                        );
                        // AUDIT line: the journal's OWN final lines, per card,
                        // per seat (journal-stated amount), next to this
                        // binary's computed award for the same (card, seat).
                        // The gate above already proved the single-pair
                        // games; this line makes EVERY diverging game's
                        // final-scoring residual checkable against BGO's
                        // own arithmetic without re-reading the journal --
                        // a game whose whole residual sits in these pairs is
                        // a journal-side record error, not an engine bug.
                        // Diverging seats carry the JOURNAL amount (that is
                        // the point: what BGO itself stated); agreeing seats
                        // carry the computed amount (journal == computed
                        // there, so the same value either way).
                        let mut journal_by_seat: std::collections::HashMap<(String, u8), i32> =
                            std::collections::HashMap::new();
                        for (card, steps) in &result.final_event_awards_computed {
                            let mut by_seat: std::collections::HashMap<u8, i32> =
                                std::collections::HashMap::new();
                            for &(seat, amount) in steps {
                                *by_seat.entry(seat).or_insert(0) += amount;
                            }
                            for (seat, amount) in by_seat {
                                let entry = journal_by_seat
                                    .entry((card.to_string(), seat))
                                    .or_insert(amount);
                                if let Some(d) = result
                                    .final_event_award_divergences
                                    .iter()
                                    .find(|d| d.card == *card && d.seat == seat)
                                {
                                    *entry = d.journal_amount;
                                }
                            }
                        }
                        for (card, steps) in &result.final_event_awards_computed {
                            let mut by_seat: std::collections::HashMap<u8, i32> =
                                std::collections::HashMap::new();
                            for &(seat, amount) in steps {
                                *by_seat.entry(seat).or_insert(0) += amount;
                            }
                            let mut seats: Vec<u8> = by_seat.keys().copied().collect();
                            seats.sort_unstable();
                            for seat in seats {
                                let j = journal_by_seat[&(card.to_string(), seat)];
                                eprintln!(
                                    "SCOREDIV_FINAL {} {} seat{} j{}/e{}",
                                    meta.id, card, seat, j, by_seat[&seat]
                                );
                            }
                        }
                    }
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
    ranked.sort_unstable_by_key(|x| std::cmp::Reverse(x.1.count));

    println!("# replaystats: {n_games} games sampled, {n_completed} completed to state.game_over\n");
    println!("## Final-score cross-check\n");
    println!(
        "{n_score_checked}/{n_completed} completed games actually flipped state.game_over (the other \
         {} reached the journal's own \"End of game\" marker but hit a mismatch afterward, so \
         `game::scores` was never computed for them); {n_score_exact}/{n_score_checked} of those matched \
         index.tsv exactly (sorted per-game score-list compare); {n_gate_suppressed} of the non-matching \
         remainder were suppressed as journal-arithmetic-error final-award divergences (the engine's \
         own final-score cross-check proves the engine right in each -- see SCOREDIV_SUPPRESSED).",
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

    // `GameResult::final_event_award_divergences` -- ranks WHICH §12.5.2
    // "Impact of ..." card's `scoring_culture` formula disagrees with BGO's
    // own journal-stated award most often, independent of grounding-order
    // noise (see that field's own doc). This is the cause-ranking instrument
    // the final-score cross-check above names a need for but cannot itself
    // supply (a final total is a SUM across every pending card; this breaks
    // it back out by card).
    println!("## Final-scoring award oracle (which \"Impact of ...\" card's formula is wrong)\n");
    println!(
        "{} games had at least one (card, seat) whose journal-stated §12.5.2 award this binary's own \
         `scoring_culture` disagreed with:\n",
        n_games_with_final_event_award_divergence
    );
    let mut final_event_ranked: Vec<(&&str, &(u32, String))> = final_event_award_buckets.iter().collect();
    final_event_ranked.sort_unstable_by_key(|b| std::cmp::Reverse(b.1.0));
    println!("| (game,seat) pairs | card | example |");
    println!("|---|---|---|");
    for (card, (count, example)) in &final_event_ranked {
        println!("| {count} | {card} | {} |", example.replace('|', "\\|"));
    }
    println!();

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

    // `GameResult::hand_ledger_verdict` -- the classification this binary
    // exists to compute: for each game's FIRST discard-phase-oracle
    // divergence above, does a PURE-JOURNAL-TEXT ledger
    // (`replay_common::prescan_military_hand_ledger`) independently
    // reproduce the journal's own truth at that checkpoint (implicating the
    // forward simulator specifically, `SimulatorBug`) or does it ALSO
    // disagree (an event class this project does not model even reading the
    // journal directly, `UnmodelledEvent`, bucketed by the most recent
    // ledger-tracked event for that actor)? Ranked so the dominant reason is
    // read first, per this file's own "measure first" convention.
    println!("## Military hand ledger: classifying WHY the discard-phase oracle diverges\n");
    let mut ledger_ranked: Vec<(&String, &(u32, String))> = ledger_verdict_buckets.iter().collect();
    ledger_ranked.sort_unstable_by_key(|x| std::cmp::Reverse(x.1.0));
    println!("| games | reason | example |");
    println!("|---|---|---|");
    for (key, (count, example)) in &ledger_ranked {
        println!("| {count} | {key} | {} |", example.replace('|', "\\|"));
    }
    println!();

    // `GameResult::culture_oracle_divergence` -- culture IS the score in
    // this game (`docs/REPLAY.md`'s "Culture-oracle" section), so this is
    // the census's own headline instrument: BGO's "(now M)" running total is
    // a PERFECT oracle (not derived, unlike the discard-phase excess above),
    // cross-validated every single "End turn" line. Ranked histogram of the
    // ActionClass immediately preceding each game's FIRST divergence -- the
    // "cause histogram" the task exists to produce.
    println!("## Culture oracle\n");
    println!(
        "{culture_oracle_checked_total} \"End turn\" checkpoints had a `\"(now M)\"` running total to check this \
         binary's own reconstructed `state.players[_].culture` against; {culture_oracle_agreed_total} \
         ({:.1}%) matched exactly.",
        100.0 * culture_oracle_agreed_total as f64 / culture_oracle_checked_total.max(1) as f64
    );
    println!(
        "{}/{n_games} games sampled had at least one checkpoint disagree (FIRST divergence only, ranked by the \
         ActionClass immediately preceding it -- a bucket name is the SYMPTOM location, not necessarily the \
         cause; trace before theorizing):\n",
        culture_cause_buckets.values().map(|(n, _)| *n as u64).sum::<u64>()
    );
    let mut culture_ranked: Vec<(&String, &(u32, String))> = culture_cause_buckets.iter().collect();
    culture_ranked.sort_unstable_by_key(|x| std::cmp::Reverse(x.1.0));
    println!("| games | preceding ActionClass | example |");
    println!("|---|---|---|");
    for (key, (count, example)) in &culture_ranked {
        println!("| {count} | {key} | {} |", example.replace('|', "\\|"));
    }
    println!();

    // `GameResult::science_oracle_divergence`/`resource_oracle_divergence` --
    // SCIDRIFT 2026-08-14 pass, `docs/REPLAY.md` never had a SCIENCE or
    // RESOURCE cross-check before this: this whole section reads 0/0 (NaN%
    // guarded to 0.0% by the `.max(1)` below) unless `SCIENCE_ORACLE`/
    // `RESOURCE_ORACLE` was set for this run -- see `debugflags::
    // science_oracle`/`resource_oracle`'s own docs for why that is the point,
    // not a bug.
    println!("## Science oracle (SCIENCE_ORACLE=1 to populate; 0/0 otherwise)\n");
    println!(
        "{science_oracle_checked_total} \"End turn\" checkpoints had a `\"(now M)\"` science running total to \
         check this binary's own reconstructed `state.players[_].science` against; {science_oracle_agreed_total} \
         ({:.1}%) matched exactly. {n_science_diverging_games}/{n_games} games sampled had at least one checkpoint \
         disagree (FIRST divergence only; see SCIENCE_DETAIL lines above).\n",
        100.0 * science_oracle_agreed_total as f64 / science_oracle_checked_total.max(1) as f64
    );
    println!("## Resource oracle (RESOURCE_ORACLE=1 to populate; 0/0 otherwise)\n");
    println!(
        "{resource_oracle_checked_total} \"End turn\" checkpoints had a `\"(now M)\"` resources running total to \
         check this binary's own reconstructed `state.players[_].resources` against; {resource_oracle_agreed_total} \
         ({:.1}%) matched exactly. {n_resource_diverging_games}/{n_games} games sampled had at least one checkpoint \
         disagree (FIRST divergence only; see RESOURCE_DETAIL lines above).\n",
        100.0 * resource_oracle_agreed_total as f64 / resource_oracle_checked_total.max(1) as f64
    );

    // `GameResult::civil_deck_premature_advance` -- docs/REPLAY.md's "civil
    // deck model" handoff. Zero here is the invariant `top_up_civil_deck`
    // is meant to guarantee; a nonzero count is this instrument catching a
    // regression, not a new investigation needed from scratch.
    println!("civil-age premature advances (this reconstruction's own age_civil read ahead of the journal's Line::age column, with more of the OLD age still to come): {n_premature} games");
    for ex in &premature_examples {
        println!("  {ex}");
    }
    println!();

    // `GameResult::politics_false_skips` -- docs/REPLAY.md's "Final scores"
    // section: the mechanism traced from the final-score cross-check above.
    // A false skip means `game::auto_skip_politics` closed a player's
    // Politics phase while the journal's own solved plan says they had a
    // real preparation waiting. `resolve_intervening` now RECOVERS every
    // one of these on the spot (reopens the phase, claims the preparation
    // through the same path an on-time one uses) -- so this is a raw
    // occurrence count for the still-open `hand_military` under-tracking
    // gap, kept nonzero ON PURPOSE as that gap's own regression signal.
    // **A nonzero value here is NOT damage** -- see `politics_false_skips`'s
    // own doc before treating a change in this number as a regression.
    // `politics_false_skips_unrecovered`, printed right after, is the real
    // "did the recovery itself break" signal and should stay at 0.
    println!(
        "politics false-skips (a real event/territory preparation the journal shows; RECOVERED in place, this is \
         an occurrence count for the still-open hand_military gap, not damage -- see GameResult::politics_false_skips's own doc): \
         {n_false_skips_total} across {n_games_with_false_skip} games"
    );
    println!(
        "politics false-skips left UNRECOVERED (the true damage signal -- should be 0): \
         {n_false_skips_unrecovered_total} across {n_games_with_unrecovered_false_skip} games\n"
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
        eprintln!("usage: replaystats <index.tsv> <journals_dir> [sample_size | --game ID ...]");
        return ExitCode::FAILURE;
    }
    let sample_size = argv.get(2).and_then(|s| s.parse().ok());
    // `--game ID ...` replays ONLY the named games, in the order given, so
    // a single-game trace does not wait on the full 1,011-game corpus.
    if argv.get(2).map(String::as_str) == Some("--game") {
        let ids: Vec<String> = argv[3..].to_vec();
        match run(&argv[0], &argv[1], None, Some(&ids)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    } else {
        match run(&argv[0], &argv[1], sample_size, None) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("error: {e}");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One fixture pair for [`journal_arithmetic_error_suppression`]'s own
    /// branches -- mirrors the field names of
    /// [`FinalEventAwardDivergence`] (a `&'static str` card name, so a real
    /// `"Impact of ..."` card's name is fine, the predicate never looks it
    /// up).
    fn gate_div(card: &'static str, seat: u8, journal: i32, engine: i32) -> FinalEventAwardDivergence {
        FinalEventAwardDivergence { card, seat, journal_amount: journal, engine_amount: engine }
    }

    #[test]
    fn scorediv_gate_suppresses_one_seat_one_pair_whole_game() {
        // 7521849's own shape: seat 1 (Purple)'s score is exactly 2 above
        // the journal's own total, and exactly one final-award pair explains
        // it (Impact of Progress, journal 12, engine 14). Every "End turn"
        // matched the engine (no culture drift). Suppressed.
        let d = gate_div("Impact of Progress", 1, 12, 14);
        // Engine (seated, Orange..Grey) = [137, 176]; Purple (seat 1) is 2
        // above the journal's own total (the pair's delta). Per-seat journal
        // totals: Orange=137, Purple=174. The index `results` column is
        // PLACEMENT-ordered (winner first): Purple 174 > Orange 137, so
        // b = [174, 137]. index_order maps placement positions to seats:
        // position 0 (174, Purple) → seat 1; position 1 (137, Orange) →
        // seat 0. So index_order = [1, 0].
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[137, 176], &b, Some(&idx), &[d], None);
        assert!(matches!(&r, Some((seat, x)) if seat == "Purple" && x.card == "Impact of Progress"));
    }

    #[test]
    fn scorediv_gate_negative_delta_is_still_suppressed() {
        // The seat's TOTAL is BELOW the journal's own total because the
        // journal OVER-stated the award (journal 14, engine 12): same shape,
        // delta -2, one seat, one pair, no drift. Suppressed.
        let d = gate_div("Impact of Progress", 1, 14, 12);
        // Journal OVER-stated (delta -2 = 12-14). Engine (seated) = [137,
        // 172]; Purple (seat 1) is 2 below the journal's own total. Per-seat
        // journal totals: Orange=137, Purple=174. b (placement-ordered) =
        // [174, 137]; index_order = [1, 0] (pos 0→seat 1, pos 1→seat 0).
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[137, 172], &b, Some(&idx), &[d], None);
        assert!(matches!(&r, Some((seat, _)) if seat == "Purple"));
    }

    #[test]
    fn scorediv_gate_direction_flip_is_never_suppressed() {
        // Journal and engine disagree on SIGN (journal "scores 12", engine
        // computes a net -4 loss): a genuine `scoring_culture` formula bug,
        // must surface as a diverging game.
        let d = gate_div("Impact of Progress", 1, 12, -4);
        // Direction flip: journal=12, engine=-4. The flip check rejects
        // before the delta comparison even matters.
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[137, 158], &b, Some(&idx), &[d], None);
        assert!(r.is_none());
    }

    #[test]
    fn scorediv_gate_two_seats_off_is_never_suppressed() {
        // Two award pairs, two seats: the deltas could cancel in a sorted
        // compare and still hide two real engine bugs. The gate demands
        // EXACTLY one pair.
        let d0 = gate_div("Impact of Progress", 0, 12, 14);
        let d1 = gate_div("Impact of Strength", 1, 10, 8);
        // Two seats off: seat 0 delta +2, seat 1 delta -2. Per-seat journal
        // totals: Orange=137, Purple=174. b (placement-ordered) = [174,
        // 137]; index_order = [1, 0].
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[139, 172], &b, Some(&idx), &[d0, d1], None);
        assert!(r.is_none());
    }

    #[test]
    fn scorediv_gate_one_pair_but_two_seats_off_is_never_suppressed() {
        // One award pair but BOTH seats' scores differ: the pair explains
        // one seat's delta, the other seat's is something else entirely.
        let d = gate_div("Impact of Progress", 1, 12, 14);
        // Both seats differ by 2, but the pair only explains seat 1. Per-seat
        // journal totals: Orange=137, Purple=174. b (placement-ordered) =
        // [174, 137]; index_order = [1, 0].
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[139, 176], &b, Some(&idx), &[d], None);
        assert!(r.is_none());
    }

    #[test]
    fn scorediv_gate_in_play_culture_drift_is_never_suppressed() {
        // The game's own culture checkpoints disagreed with the engine
        // somewhere (a real in-play bug), so "the engine is right" is not
        // proven for this game: even the cleanest one-pair shape must
        // surface.
        let d = gate_div("Impact of Progress", 1, 12, 14);
        let drifted = CultureOracleDivergence {
            lineno: 367,
            actor: "Purple",
            journal_now: 128,
            reconstructed: 129,
            last_action_class: None,
        };
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[137, 175], &b, Some(&idx), &[d], Some(&drifted));
        assert!(r.is_none());
    }

    #[test]
    fn scorediv_gate_seat_mismatch_between_pair_and_lists_is_never_suppressed() {
        // The one pair's delta matches ONE seat's difference, but the pair's
        // own SEAT is the OTHER one: the journal's own line is consistent
        // with the engine for the pair's seat, so the pair is not the
        // error. Must surface.
        let d = gate_div("Impact of Progress", 1, 12, 14);
        // Seat 0's score is the one off by 2; the pair says seat 1. Per-seat
        // journal totals: Orange=137, Purple=174. b (placement-ordered) =
        // [174, 137]; index_order = [1, 0].
        let b = [174, 137];
        let idx = [1u8, 0];
        let r = journal_arithmetic_error_suppression(&[139, 174], &b, Some(&idx), &[d], None);
        assert!(r.is_none());
    }

    #[test]
    fn scorediv_gate_seats_index_by_placement_not_by_sorted_position() {
        // The whole reason the gate took an `index_order` argument:
        // index.tsv's `results` column is in PLACEMENT order (winner
        // first), NOT seat order, so the sorted position is NOT the seat
        // index. 7523253's own shape: winner (Green, seat 2) is first in
        // the index; the engine's only error is a one-pair +2 on Green.
        // a (seated, Orange..Grey) = [215, 199, 242, 198];
        // b (index, placement: Green 240, Orange 215, Purple 199, Grey 198)
        // = [240, 215, 199, 198]; the pair's delta (+2) and seat (2, Green)
        // match Green's seated diff exactly. Must suppress.
        let d = gate_div("Impact of Happiness", 2, 10, 12);
        // 7523253's own shape. Per-seat journal totals: Orange=215,
        // Purple=199, Green=240 (winner), Grey=198. The index `results`
        // column is PLACEMENT-ordered (winner first): [240, 215, 199, 198]
        // -- a permutation of the seated list that is NOT the identity. The
        // gate must seat b by the derived index_order [2,0,1,3], NOT assume
        // the sorted position is the seat index. Engine (seated) =
        // [215, 199, 242, 198]: Green (seat 2) is the +2 error.
        let b = [240, 215, 199, 198];
        let a = [215, 199, 242, 198];
        let idx = [2u8, 0, 1, 3];
        let r = journal_arithmetic_error_suppression(&a, &b, Some(&idx), &[d.clone()], None);
        assert!(matches!(&r, Some((seat, _)) if seat == "Green"));
        // And the same game is NOT suppressed without a pinned winner:
        // the gate cannot seat `b` and must decline rather than guess.
        let none_order = journal_arithmetic_error_suppression(&a, &b, None, &[d.clone()], None);
        assert!(none_order.is_none());
    }

    #[test]
    fn hand_ledger_verdict_key_names_the_mechanism_for_simulator_bug_via_the_passed_in_last_event() {
        // `SimulatorBug`'s own variant carries no event -- the caller passes
        // `DiscardOracleDivergence::ledger_last_event` in separately, and
        // the key must still name it, not fall back to a bare "SimulatorBug"
        // that loses the one clue pointing at which call site is buggy.
        let key = hand_ledger_verdict_key(&HandLedgerVerdict::SimulatorBug, Some(LedgerEventKind::PrepareEvent));
        assert!(key.contains("SimulatorBug"));
        assert!(key.contains("PrepareEvent"));
    }

    #[test]
    fn hand_ledger_verdict_key_names_the_mechanism_for_an_unmodelled_event() {
        let key = hand_ledger_verdict_key(&HandLedgerVerdict::UnmodelledEvent(Some(LedgerEventKind::Discard)), None);
        assert!(key.contains("UnmodelledEvent"));
        assert!(key.contains("Discard"));
    }

    #[test]
    fn hand_ledger_verdict_key_distinguishes_simulator_bug_from_unmodelled_event_with_the_same_last_kind() {
        // The two verdicts mean OPPOSITE things (ledger right vs ledger also
        // wrong) for the exact same preceding mechanism -- must never
        // collapse to the same bucket key.
        let a = hand_ledger_verdict_key(&HandLedgerVerdict::SimulatorBug, Some(LedgerEventKind::Draw));
        let b = hand_ledger_verdict_key(&HandLedgerVerdict::UnmodelledEvent(Some(LedgerEventKind::Draw)), Some(LedgerEventKind::Draw));
        assert_ne!(a, b);
    }

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
