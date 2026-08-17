//! `cardblame` -- ranks cards, and now separately every kind of THING THAT
//! HAPPENED, by how much each ENRICHES a game's chance of failing, instead
//! of chasing failures bucket-by-bucket by symptom (the approach
//! `replaystats` implements, and which keeps landing on shared, hard-to-fix
//! mechanisms). The idea: a card whose implementation is simply WRONG
//! should show up as a statistical outlier -- games it touched fail far
//! more often than the corpus base rate -- even before anyone knows which
//! mechanism is broken. This already worked once by accident: Age II
//! `Fortifications` was baked into `card_table.rs` at the wrong strength
//! (4/2 instead of BGA's printed 5/3), and fixing that one constant won 13
//! games outright.
//!
//! ```text
//! cargo run --profile difftest --bin cardblame -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals [sample_size]
//! ```
//!
//! Same corpus, same replay (`tta::replay_common::replay_game`), same
//! sampling convention as `replaystats` (no shuffling, `sample_size` takes
//! the first N games in `index.tsv` order). This binary is purely
//! additive: every field it reads off `GameResult` (`cards_in_play`,
//! `ever_cards_in_play`, `moves_applied`, `line_classes`,
//! `corruption_checks`, all documented at their own definitions in
//! `replay_common.rs`) is always-on and read-only; nothing here changes
//! what any existing binary computes or prints -- see `replaystats`' own
//! byte-identical proof this task shipped alongside this file.
//!
//! # Two attribution sets, kept side by side (task 2026-08-14 part A)
//!
//! `GameResult::cards_in_play` ("narrow") is a SNAPSHOT: all players'
//! tableaus, plus whichever leader/tactic/government slot is occupied at
//! the point a game stopped or ended. `GameResult::ever_cards_in_play`
//! ("widened") is everything the narrow set has PLUS every card this
//! reconstruction ever saw PLAYED or RESOLVED at any point of the game --
//! event cards (which never sit in a tableau at all), one-shot action
//! cards, aggressions/wars/pacts, and superseded leaders/tactics/
//! governments a single end-of-game snapshot cannot see. They answer
//! DIFFERENT questions (a "still present" card vs. a "was exercised" card)
//! and neither is a replacement for the other -- both are printed, in full,
//! clearly labelled `[narrow]` / `[widened]`.
//!
//! Each set has its OWN confound (see the module doc for each field in
//! `replay_common.rs` for the full explanation, condensed here):
//!   - narrow: a single-slot card (leader/tactic/government) present at a
//!     COMPLETED game's final snapshot is definitionally the LAST one
//!     taken -- an early one can only appear in a game that never reached
//!     the point of being superseded, which for a full-length game is
//!     itself a signal the card is short-lived, not that it is broken.
//!   - widened: a card played in round 3 stays in this set for the rest of
//!     the game, so a LONGER game accumulates strictly MORE of these by
//!     construction -- the opposite bias from the narrow set's.
//!
//! Both biases are round-length proxies. `round_with`/`round_without` are
//! printed on every ranked row of both sets for exactly this reason.
//!
//! # Total coverage (task 2026-08-14 "fix it to be everything")
//!
//! Every card in [`tta::CARDS`] (indices `0..NUM_CARDS`, not a hand-picked
//! list) gets a row somewhere in each card cut: RANKED (cleared
//! `MIN_EXPOSURE`), LOW EXPOSURE (seen, but too rarely to trust a rate), or
//! ZERO EXPOSURE (never once appeared in this set across the whole sample
//! -- itself a finding: either the replayer never exercises it, or BGO
//! journals never named it in this corpus). A card is never silently
//! dropped from the output.
//!
//! The happenings cuts are built the same way, off TWO independent,
//! EXHAUSTIVE classifications with no wildcard arm (this file's own lint
//! gate, `wildcard_enum_match_arm`, denies `_ =>` outright, so this is
//! compiler-enforced, not just a convention):
//!   - [`move_kind_name`]/[`move_card_args`] match every [`tta::Move`]
//!     variant by name. A future variant added to `moves.rs` fails to
//!     compile here until someone decides both its display name and which
//!     card(s) (if any) it names for the widened card set.
//!   - `apply_one`'s own `class: ActionClass` (BGO's own journal-line
//!     classification, independent of `Move` -- several variants like
//!     `WinAuction`/`WinWar`/`PlayEvent`/`Discard` are pure confirmation
//!     lines with no `Move` of their own) is captured into
//!     `GameResult::line_classes` every time, and `ActionClass::ALL` (the
//!     crate's own exhaustiveness-checked constant) is what this file
//!     pre-seeds its counters from, so a variant with ZERO occurrences
//!     across the whole sample still prints a row instead of silently
//!     never appearing.
//!
//! A "coverage summary" line is printed FIRST, before any ranking: how
//! many distinct cards/Move variants/ActionClass variants exist, how many
//! were observed at all, and how many cleared the exposure cutoff, for
//! both the narrow and widened card sets. A "not attributable" section
//! follows it, stating explicitly what this tool structurally cannot see,
//! rather than letting a gap show up only as silence.
//!
//! # Statistics discipline (the whole point of this file)
//!
//! A raw count is useless -- common cards/happenings appear in nearly
//! every game and top any raw list by construction. Every ranked row
//! divides by its own exposure (`games`, printed alongside the rate) and
//! compares against the corpus base rate (`enrichment = rate / base_rate`,
//! 1.0 = no signal). `MIN_EXPOSURE` (20 games/checkpoints) gates the
//! ranked-by-enrichment table; rows below it are NOT dropped, they print
//! in a separate low-exposure section so a reader can still see them
//! without being misled by their own noise into thinking they rank.
//! Nothing is truncated to a top-N any more (task 2026-08-14 part 3):
//! every row that exists prints, grouped by section instead.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::ActionClass;
use tta::replay_common::{build_card_index, replay_game};
use tta::{CardId, CardType, Move, NUM_CARDS};

/// Below this many games/checkpoints of exposure, a row's own rate is noise
/// (present in 6 games with 6 failures says nothing) -- gates the
/// ranked-by-enrichment tables, but every row below it still prints in a
/// separate low-exposure section rather than being dropped outright, and a
/// row with literally zero exposure gets its OWN section rather than being
/// folded into "low exposure" (see the module doc's "Total coverage").
const MIN_EXPOSURE: u32 = 20;

/// Every [`Move`] variant's own display name -- exhaustive, no wildcard, so
/// a future variant added to `moves.rs` is a compile error here until
/// someone gives it a name. Kept as a plain `match` (not a `Debug` format)
/// because `Move`'s own `Debug` prints field values too (`Take { slot: 3
/// }`), which would fragment one variant into many distinct counter keys --
/// this file wants ONE counter per variant, not one per (variant, args).
fn move_kind_name(mv: Move) -> &'static str {
    use tta::moves::Move::*;
    match mv {
        Take { .. } => "Take",
        Build { .. } => "Build",
        Develop { .. } => "Develop",
        Upgrade { .. } => "Upgrade",
        WonderStep { .. } => "WonderStep",
        Pop => "Pop",
        PopFree => "PopFree",
        Revolution { .. } => "Revolution",
        PlayLeader { .. } => "PlayLeader",
        PlayAction { .. } => "PlayAction",
        Destroy { .. } => "Destroy",
        PlayTactic { .. } => "PlayTactic",
        CopyTactic { .. } => "CopyTactic",
        Aggression { .. } => "Aggression",
        War { .. } => "War",
        OfferPact { .. } => "OfferPact",
        CancelPact { .. } => "CancelPact",
        PrepareEvent { .. } => "PrepareEvent",
        RemoveLeaderYellow => "RemoveLeaderYellow",
        ColumbusColonize { .. } => "ColumbusColonize",
        Barbarossa { .. } => "Barbarossa",
        BachTheater { .. } => "BachTheater",
        TradeFoodAsResource => "TradeFoodAsResource",
        TradeResourceAsFood => "TradeResourceAsFood",
        Bid { .. } => "Bid",
        BidPass => "BidPass",
        Defend { .. } => "Defend",
        DefendDone => "DefendDone",
        SendUnit { .. } => "SendUnit",
        SendBonus { .. } => "SendBonus",
        SendDiscard { .. } => "SendDiscard",
        SendDone => "SendDone",
        Choose { .. } => "Choose",
        Churchill { .. } => "Churchill",
        EndTurn => "EndTurn",
        PolPass => "PolPass",
        Resign => "Resign",
    }
}

/// Every name [`move_kind_name`] can produce, in the SAME order as
/// `moves.rs` declares the variants -- used only to pre-seed the
/// happenings counters so a Move variant with ZERO occurrences in the
/// sample still prints a row (see the module doc's "Total coverage").
/// **Kept in sync with `move_kind_name`/`move_card_args` by hand** (`Move`
/// has no built-in variant iterator, since several variants carry data);
/// the two `match` blocks those functions use ARE compiler-enforced
/// exhaustive, so a new variant fails to build until it is added there --
/// add it here too at the same time, `move_kind_names_has_no_duplicates_
/// and_has_the_full_variant_count` below is a tripwire, not a guarantee.
const ALL_MOVE_KIND_NAMES: &[&str] = &[
    "Take",
    "Build",
    "Develop",
    "Upgrade",
    "WonderStep",
    "Pop",
    "PopFree",
    "Revolution",
    "PlayLeader",
    "PlayAction",
    "Destroy",
    "PlayTactic",
    "CopyTactic",
    "Aggression",
    "War",
    "OfferPact",
    "CancelPact",
    "PrepareEvent",
    "RemoveLeaderYellow",
    "ColumbusColonize",
    "Barbarossa",
    "BachTheater",
    "TradeFoodAsResource",
    "TradeResourceAsFood",
    "Bid",
    "BidPass",
    "Defend",
    "DefendDone",
    "SendUnit",
    "SendBonus",
    "SendDiscard",
    "SendDone",
    "Choose",
    "Churchill",
    "EndTurn",
    "PolPass",
    "Resign",
];

/// Which printed GROUP a card's own [`CardType`] belongs to, for the
/// widened card cut's `group` column -- exhaustive, no wildcard, so a
/// future `CardType` added to `cards.rs` (itself already exhaustive by
/// construction against `data/*.json`, see that enum's own doc) is a
/// compile error here until someone decides which group it prints under.
fn card_group(kind: CardType) -> &'static str {
    use CardType::*;
    match kind {
        Farm | Mine => "production",
        Lab | Temple | Library | Arena | Theater => "urban building",
        Infantry | Cavalry | Artillery | Air => "military unit",
        Government => "government",
        SpecialTech => "special technology",
        Wonder => "wonder",
        Leader => "leader",
        Action => "action (one-shot)",
        Tactic => "tactic",
        Aggression => "aggression",
        War => "war",
        Pact => "pact",
        Bonus => "military bonus",
        Territory => "territory / colony",
        Event => "event",
    }
}

fn group_of_card_name(name: &str) -> &'static str {
    match CardId::by_name(name) {
        Some(id) => card_group(id.kind()),
        None => "?", // unreachable in practice: every key here came from CardId::name() itself
    }
}

/// Running totals for one row's exposure to one binary outcome (did the
/// game fail / did the score mismatch), plus the round-reached sums needed
/// to report the late-game confound alongside the rate -- see the module
/// doc's "Two attribution sets" section.
#[derive(Default, Clone, Copy)]
struct CardCounter {
    games: u32,
    failures: u32,
    round_sum: u64,
}

impl CardCounter {
    fn rate(&self) -> f64 {
        self.failures as f64 / self.games.max(1) as f64
    }

    fn mean_round(&self) -> f64 {
        self.round_sum as f64 / self.games.max(1) as f64
    }
}

/// One ranked row: a card/happening's own exposure/failure counts against
/// the corpus base rate, plus the confound-visibility figures and its
/// printed group (cards only -- happenings rows leave this `""`). Kept as
/// a struct (not printed inline while iterating a `HashMap`) so it can be
/// SORTED by enrichment before printing -- `HashMap` iteration order is
/// not that order.
struct Ranked<'a> {
    name: &'a str,
    group: &'static str,
    games: u32,
    failures: u32,
    rate: f64,
    enrichment: f64,
    round_with: f64,
    round_without: f64,
}

fn rank_cards<'a>(
    counters: &'a HashMap<&'static str, CardCounter>,
    base_rate: f64,
    total_games: u32,
    total_round_sum: u64,
    group_of: &dyn Fn(&str) -> &'static str,
) -> (Vec<Ranked<'a>>, Vec<Ranked<'a>>) {
    let mut ranked = Vec::new();
    let mut low_exposure = Vec::new();
    for (&name, c) in counters {
        let round_without = if total_games > c.games {
            (total_round_sum - c.round_sum) as f64 / (total_games - c.games) as f64
        } else {
            f64::NAN // every sampled game had this in play -- no "without" group exists
        };
        let row = Ranked {
            name,
            group: group_of(name),
            games: c.games,
            failures: c.failures,
            rate: c.rate(),
            enrichment: c.rate() / base_rate.max(f64::MIN_POSITIVE),
            round_with: c.mean_round(),
            round_without,
        };
        if c.games >= MIN_EXPOSURE {
            ranked.push(row);
        } else if c.games > 0 {
            low_exposure.push(row);
        }
        // c.games == 0 rows are printed separately by `print_zero_exposure`
        // (cards) or fall out of `ALL_MOVE_KIND_NAMES`/`ActionClass::ALL`
        // pre-seeding directly (happenings) -- not pushed to either list
        // here, so a reader does not have to tell "0 games, printed as
        // noise" apart from "genuinely never observed" by eye.
    }
    ranked.sort_unstable_by(|a, b| b.enrichment.total_cmp(&a.enrichment));
    low_exposure.sort_unstable_by_key(|b| std::cmp::Reverse(b.games));
    (ranked, low_exposure)
}

fn print_ranked(title: &str, base_rate: f64, base_num: u32, base_den: u32, ranked: &[Ranked], low_exposure: &[Ranked], show_group: bool) {
    println!("## {title}\n");
    println!(
        "corpus base rate: {base_num}/{base_den} = {:.1}% -- every row's own rate below is divided by this to get \
         `enrichment` (1.0 = exactly base rate, no signal)\n",
        100.0 * base_rate
    );
    println!(
        "ALL rows clearing the {MIN_EXPOSURE}-exposure cutoff, by enrichment descending (not capped to a top-N -- \
         see the module doc's \"Total coverage\"; `round_with`/`round_without` are the late-game confound check -- \
         a big gap there means treat `enrichment` as partly a game-length proxy, not purely a card/happening \
         effect):\n"
    );
    if show_group {
        println!("| enrichment | name | group | games | failures | rate | round_with | round_without |");
        println!("|---|---|---|---|---|---|---|---|");
    } else {
        println!("| enrichment | name | games | failures | rate | round_with | round_without |");
        println!("|---|---|---|---|---|---|---|");
    }
    for row in ranked {
        if show_group {
            println!(
                "| {:.2}x | {} | {} | {} | {} | {:.1}% | {:.1} | {:.1} |",
                row.enrichment, row.name, row.group, row.games, row.failures, 100.0 * row.rate, row.round_with, row.round_without
            );
        } else {
            println!(
                "| {:.2}x | {} | {} | {} | {:.1}% | {:.1} | {:.1} |",
                row.enrichment, row.name, row.games, row.failures, 100.0 * row.rate, row.round_with, row.round_without
            );
        }
    }
    println!(
        "\n{} rows below the {MIN_EXPOSURE}-exposure cutoff but seen at least once (listed, not hidden -- their \
         rate is noise, but noise is not the same as nothing):\n",
        low_exposure.len()
    );
    if show_group {
        println!("| name | group | games | failures | rate |");
        println!("|---|---|---|---|---|");
    } else {
        println!("| name | games | failures | rate |");
        println!("|---|---|---|---|");
    }
    for row in low_exposure {
        if show_group {
            println!("| {} | {} | {} | {} | {:.1}% |", row.name, row.group, row.games, row.failures, 100.0 * row.rate);
        } else {
            println!("| {} | {} | {} | {:.1}% |", row.name, row.games, row.failures, 100.0 * row.rate);
        }
    }
    println!();
}

/// Every card in [`tta::CARDS`] that this `counters` map has NO entry for
/// at all -- i.e. zero exposure across the whole sample, in THIS
/// particular set (narrow or widened). Printed as its own section (task
/// 2026-08-14 part 1): a card that never appears is itself a finding (it
/// may mean the replayer never exercises it, or that this corpus's own
/// sample never happened to, or -- worth checking by hand if it surprises
/// a reader -- that the card is unimplemented).
fn print_zero_exposure(title: &str, counters: &HashMap<&'static str, CardCounter>) {
    let mut zero: Vec<(&'static str, &'static str)> = Vec::new();
    for i in 0..NUM_CARDS {
        let id = CardId(i as u16);
        let name = id.name();
        if !counters.contains_key(name) {
            zero.push((name, card_group(id.kind())));
        }
    }
    zero.sort_unstable();
    println!(
        "{} of {NUM_CARDS} cards have ZERO exposure in this set ({title}) -- never appears across the whole \
         sample:\n",
        zero.len()
    );
    println!("| card | group |");
    println!("|---|---|");
    for (name, group) in &zero {
        println!("| {name} | {group} |");
    }
    println!();
}

/// One game's own contribution to a `HashMap<&'static str, CardCounter>`
/// happenings/card cut: `names` is the DEDUPLICATED set of keys this game
/// touched (a card built by both players, or a Move applied on turn 3 and
/// turn 30 both, counts once per game -- same "was this in play at all"
/// convention `cards_in_play`/`ever_cards_in_play` already use).
fn accumulate<'a>(counters: &mut HashMap<&'a str, CardCounter>, names: impl IntoIterator<Item = &'a str>, round_reached: u32, failed: bool) {
    for name in names {
        let c = counters.entry(name).or_default();
        c.games += 1;
        c.round_sum += round_reached as u64;
        if failed {
            c.failures += 1;
        }
    }
}

fn run(index_path: &str, journals_dir: &str, sample_size: Option<usize>) -> Result<(), String> {
    let card_index = build_card_index();
    let mut games = tta::corpus::parse_index(index_path)?;
    if let Some(n) = sample_size {
        games.truncate(n);
    }

    let mut n_games = 0u32;
    let mut n_completed = 0u32;
    let mut n_score_checked = 0u32;
    let mut n_score_exact = 0u32;
    let mut total_round_sum = 0u64; // completion-failure cuts' own denominator
    let mut total_round_sum_completed = 0u64; // score-mismatch cuts' own denominator (completed games only)

    let mut completion_narrow: HashMap<&'static str, CardCounter> = HashMap::new();
    let mut completion_widened: HashMap<&'static str, CardCounter> = HashMap::new();
    let mut mismatch_narrow: HashMap<&'static str, CardCounter> = HashMap::new();
    let mut mismatch_widened: HashMap<&'static str, CardCounter> = HashMap::new();

    let mut completion_move: HashMap<&'static str, CardCounter> = HashMap::new();
    let mut mismatch_move: HashMap<&'static str, CardCounter> = HashMap::new();
    let mut completion_class: HashMap<&'static str, CardCounter> = HashMap::new();
    let mut mismatch_class: HashMap<&'static str, CardCounter> = HashMap::new();
    // Pre-seed BOTH happenings maps with every variant at zero exposure --
    // see the module doc's "Total coverage": a Move/ActionClass variant
    // that never once fires in the sample must still print a row, not
    // silently vanish for want of a HashMap entry.
    for &name in ALL_MOVE_KIND_NAMES {
        completion_move.entry(name).or_default();
        mismatch_move.entry(name).or_default();
    }
    for class in ActionClass::ALL {
        completion_class.entry(class.label()).or_default();
        mismatch_class.entry(class.label()).or_default();
    }

    // Cut 3's own totals -- unchanged from the previous pass, kept
    // separate from the widening/happenings work above (see the "not
    // attributable" section of the printed output for why).
    let mut corruption_checks_total = 0u64;
    let mut corruption_disagreements_total = 0u64;
    let mut corruption_counters: HashMap<&'static str, CardCounter> = HashMap::new();

    for meta in &games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let Ok(text) = fs::read_to_string(&path) else {
            continue; // no journal file for this id -- skip, don't count, matches replaystats
        };
        n_games += 1;
        let result = replay_game(meta, &text, &card_index, false);

        let round_reached: u32 = if result.completed {
            meta.rounds
        } else {
            result.mismatch.as_ref().and_then(|m| m.round.parse().ok()).unwrap_or(0)
        };
        total_round_sum += round_reached as u64;
        let failed = !result.completed;

        accumulate(&mut completion_narrow, result.cards_in_play.iter().copied(), round_reached, failed);
        accumulate(&mut completion_widened, result.ever_cards_in_play.iter().copied(), round_reached, failed);

        let move_names: HashSet<&'static str> = result.moves_applied.iter().map(|&mv| move_kind_name(mv)).collect();
        accumulate(&mut completion_move, move_names.iter().copied(), round_reached, failed);
        let class_names: HashSet<&'static str> = result.line_classes.iter().map(|c| c.label()).collect();
        accumulate(&mut completion_class, class_names.iter().copied(), round_reached, failed);

        if result.completed {
            n_completed += 1;
            total_round_sum_completed += round_reached as u64;
            if let Some(engine) = &result.engine_scores {
                n_score_checked += 1;
                let mut a = engine.clone();
                let mut b = result.index_scores.clone();
                a.sort_unstable();
                b.sort_unstable();
                let exact = a == b;
                if exact {
                    n_score_exact += 1;
                }
                let mismatched = !exact;
                accumulate(&mut mismatch_narrow, result.cards_in_play.iter().copied(), round_reached, mismatched);
                accumulate(&mut mismatch_widened, result.ever_cards_in_play.iter().copied(), round_reached, mismatched);
                accumulate(&mut mismatch_move, move_names.iter().copied(), round_reached, mismatched);
                accumulate(&mut mismatch_class, class_names.iter().copied(), round_reached, mismatched);
            }
        }

        for check in &result.corruption_checks {
            corruption_checks_total += 1;
            let disagrees = check.engine_charges != check.journal_charges;
            if disagrees {
                corruption_disagreements_total += 1;
            }
            for &card in &check.cards_in_play {
                let c = corruption_counters.entry(card).or_default();
                c.games += 1; // one "game" unit here is really one checkpoint -- see printed header
                if disagrees {
                    c.failures += 1;
                }
            }
        }
    }

    println!("# cardblame: {n_games} games sampled, {n_completed} completed, {n_score_exact}/{n_score_checked} exact\n");

    // ---- coverage summary (task 2026-08-14 part 3) ----
    let base_failure_rate = (n_games - n_completed) as f64 / n_games.max(1) as f64;
    let base_mismatch_rate = (n_score_checked - n_score_exact) as f64 / n_score_checked.max(1) as f64;

    let (ranked_cn, low_cn) = rank_cards(&completion_narrow, base_failure_rate, n_games, total_round_sum, &group_of_card_name);
    let (ranked_cw, low_cw) = rank_cards(&completion_widened, base_failure_rate, n_games, total_round_sum, &group_of_card_name);
    let (ranked_mn, low_mn) = rank_cards(&mismatch_narrow, base_mismatch_rate, n_completed, total_round_sum_completed, &group_of_card_name);
    let (ranked_mw, low_mw) = rank_cards(&mismatch_widened, base_mismatch_rate, n_completed, total_round_sum_completed, &group_of_card_name);
    let empty_group = |_: &str| "";
    let (ranked_move_c, low_move_c) = rank_cards(&completion_move, base_failure_rate, n_games, total_round_sum, &empty_group);
    let (ranked_move_m, low_move_m) =
        rank_cards(&mismatch_move, base_mismatch_rate, n_completed, total_round_sum_completed, &empty_group);
    let (ranked_class_c, low_class_c) = rank_cards(&completion_class, base_failure_rate, n_games, total_round_sum, &empty_group);
    let (ranked_class_m, low_class_m) =
        rank_cards(&mismatch_class, base_mismatch_rate, n_completed, total_round_sum_completed, &empty_group);

    println!("## Coverage summary\n");
    println!(
        "cards: {NUM_CARDS} total in `tta::CARDS`. narrow set (cards_in_play): {} observed at least once, {} \
         cleared the {MIN_EXPOSURE}-game cutoff. widened set (ever_cards_in_play): {} observed at least once, {} \
         cleared the cutoff.",
        completion_narrow.len(),
        ranked_cn.len(),
        completion_widened.len(),
        ranked_cw.len(),
    );
    println!(
        "Move variants: {} total (`moves.rs`). {} observed at least once, {} cleared the cutoff (completion cut).",
        ALL_MOVE_KIND_NAMES.len(),
        completion_move.values().filter(|c| c.games > 0).count(),
        ranked_move_c.len(),
    );
    println!(
        "ActionClass variants: {} total (`corpus::ActionClass::ALL`). {} observed at least once, {} cleared the \
         cutoff (completion cut).\n",
        ActionClass::ALL.len(),
        completion_class.values().filter(|c| c.games > 0).count(),
        ranked_class_c.len(),
    );

    println!("## Not attributable by this tool\n");
    println!(
        "- Anything strictly AFTER a game's replay STOPS on a `Mismatch`: only what the replayer actually \
         reconstructed up to the stop point is visible to any cut here. A card or happening that would only have \
         mattered LATER in that same game is invisible by construction, not merely unsampled.\n\
         - Per-turn passive/continuous card effects with no `Move` or `ActionClass` line of their own (e.g. a \
         `Special` like `CulturePerLabEqualToLevel`, folded silently into whichever `Move` triggers scoring, \
         usually `EndTurn`): the OWNING card still lands in `cards_in_play`/`ever_cards_in_play` so it can still be \
         blamed at the card level, but there is no separate happenings row for the effect itself.\n\
         - Cut 3 (corruption vs. journal `CORRUPTION!` marker) is its OWN separate checkpoint-level population, \
         kept exactly as previously built -- not re-run against the widened card set or the happenings cuts this \
         pass.\n\
         - `GameResult::final_event_cards`/`final_event_award_divergences` (the separate §12.5.2 final-scoring \
         oracle) are not folded into any cut here.\n"
    );

    print_ranked(
        "Cut 1 [narrow]: completion-failure enrichment -- cards_in_play (snapshot at stop/end)",
        base_failure_rate,
        n_games - n_completed,
        n_games,
        &ranked_cn,
        &low_cn,
        true,
    );
    print_zero_exposure("Cut 1 [narrow]", &completion_narrow);

    print_ranked(
        "Cut 1w [widened]: completion-failure enrichment -- ever_cards_in_play (played or resolved at any point)",
        base_failure_rate,
        n_games - n_completed,
        n_games,
        &ranked_cw,
        &low_cw,
        true,
    );
    print_zero_exposure("Cut 1w [widened]", &completion_widened);

    print_ranked(
        "Cut 2 [narrow]: score-mismatch enrichment (completing games only) -- cards_in_play",
        base_mismatch_rate,
        n_score_checked - n_score_exact,
        n_score_checked,
        &ranked_mn,
        &low_mn,
        true,
    );
    print_zero_exposure("Cut 2 [narrow]", &mismatch_narrow);

    print_ranked(
        "Cut 2w [widened]: score-mismatch enrichment (completing games only) -- ever_cards_in_play",
        base_mismatch_rate,
        n_score_checked - n_score_exact,
        n_score_checked,
        &ranked_mw,
        &low_mw,
        true,
    );
    print_zero_exposure("Cut 2w [widened]", &mismatch_widened);

    print_ranked(
        "Cut 4: happenings by Move -- completion-failure enrichment (every Move variant, exhaustively classified)",
        base_failure_rate,
        n_games - n_completed,
        n_games,
        &ranked_move_c,
        &low_move_c,
        false,
    );
    print_ranked(
        "Cut 5: happenings by Move -- score-mismatch enrichment (completing games only)",
        base_mismatch_rate,
        n_score_checked - n_score_exact,
        n_score_checked,
        &ranked_move_m,
        &low_move_m,
        false,
    );
    print_ranked(
        "Cut 6: happenings by ActionClass -- completion-failure enrichment (every journal line-shape, exhaustively classified)",
        base_failure_rate,
        n_games - n_completed,
        n_games,
        &ranked_class_c,
        &low_class_c,
        false,
    );
    print_ranked(
        "Cut 7: happenings by ActionClass -- score-mismatch enrichment (completing games only)",
        base_mismatch_rate,
        n_score_checked - n_score_exact,
        n_score_checked,
        &ranked_class_m,
        &low_class_m,
        false,
    );

    println!("## Cut 3: corruption charge vs journal `CORRUPTION!` marker (unchanged from the previous pass)\n");
    println!(
        "{corruption_disagreements_total}/{corruption_checks_total} \"End turn\" checkpoints disagree on whether \
         corruption was charged this turn (both agreements AND disagreements counted here -- a card equally common \
         on both sides is NOT a cause and this cut is built to say so, not just to find something).\n"
    );
    println!(
        "each row's `games` is CHECKPOINTS (one per \"End turn\" line), not games -- a card in play for many turns \
         of the same game is counted once per turn, on purpose: the hypothesis under test (a permanent -1 \
         blue-token territory) is a per-turn effect, not a per-game one. Uses the ACTING PLAYER's own broader card \
         set (tableau, government, leader, wonder, tactic, and every territory won at colonization), NOT the \
         narrow or widened sets above -- see `player_cards_in_play`'s own doc in `replay_common.rs`.\n"
    );
    let base_corruption_rate = corruption_disagreements_total as f64 / corruption_checks_total.max(1) as f64;
    let (ranked, low_exposure) =
        rank_cards(&corruption_counters, base_corruption_rate, corruption_checks_total as u32, 0, &group_of_card_name);
    println!(
        "corpus base disagreement rate: {corruption_disagreements_total}/{corruption_checks_total} = {:.1}%\n",
        100.0 * base_corruption_rate
    );
    println!(
        "all rows clearing {MIN_EXPOSURE} checkpoints (the round-reached confound columns from the card cuts above \
         are omitted here -- this cut counts per-TURN checkpoints, not per-game, so a per-game round-reached figure \
         would not mean the same thing):\n"
    );
    println!("| enrichment | card | group | checkpoints | disagreements | rate |");
    println!("|---|---|---|---|---|---|");
    for row in &ranked {
        println!(
            "| {:.2}x | {} | {} | {} | {} | {:.1}% |",
            row.enrichment,
            row.name,
            row.group,
            row.games,
            row.failures,
            100.0 * row.rate
        );
    }
    println!("\n{} cards below the {MIN_EXPOSURE}-checkpoint exposure cutoff:\n", low_exposure.len());
    println!("| card | group | checkpoints | disagreements | rate |");
    println!("|---|---|---|---|---|");
    for row in &low_exposure {
        println!("| {} | {} | {} | {} | {:.1}% |", row.name, row.group, row.games, row.failures, 100.0 * row.rate);
    }

    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 2 {
        eprintln!("usage: cardblame <index.tsv> <journals_dir> [sample_size]");
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
    fn card_counter_rate_is_zero_for_a_card_with_no_recorded_games_rather_than_a_division_panic() {
        let c = CardCounter::default();
        assert_eq!(c.rate(), 0.0);
    }

    #[test]
    fn card_counter_rate_is_the_plain_failures_over_games_ratio() {
        let c = CardCounter { games: 4, failures: 1, round_sum: 40 };
        assert_eq!(c.rate(), 0.25);
    }

    #[test]
    fn card_counter_mean_round_divides_the_round_sum_by_games_not_failures() {
        let c = CardCounter { games: 4, failures: 1, round_sum: 40 };
        assert_eq!(c.mean_round(), 10.0);
    }

    #[test]
    fn rank_cards_computes_enrichment_as_the_cards_own_rate_over_the_base_rate() {
        let mut counters = HashMap::new();
        // Card in play for 25 games (over MIN_EXPOSURE), fails in 10 of
        // them -- a 40% rate against a 20% base is a clean 2x.
        counters.insert("Suspect Card", CardCounter { games: 25, failures: 10, round_sum: 250 });
        let (ranked, low_exposure) = rank_cards(&counters, 0.20, 100, 1000, &|_| "");
        assert!(low_exposure.is_empty(), "25 games clears the 20-game MIN_EXPOSURE cutoff");
        assert_eq!(ranked.len(), 1);
        assert!((ranked[0].enrichment - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rank_cards_routes_a_card_below_the_exposure_cutoff_to_the_low_exposure_list_not_the_ranked_one() {
        let mut counters = HashMap::new();
        counters.insert("Rare Card", CardCounter { games: 6, failures: 6, round_sum: 60 });
        let (ranked, low_exposure) = rank_cards(&counters, 0.20, 100, 1000, &|_| "");
        assert!(ranked.is_empty(), "6 games in play must not clear a 20-game minimum");
        assert_eq!(low_exposure.len(), 1);
        assert_eq!(low_exposure[0].name, "Rare Card");
    }

    #[test]
    fn rank_cards_omits_a_zero_exposure_card_from_both_the_ranked_and_low_exposure_lists() {
        // A pre-seeded happenings entry that never fired this sample --
        // must not show up as a fake "0 games, 0.0% rate" low-exposure
        // row (that would be indistinguishable from a real but rare one).
        let mut counters = HashMap::new();
        counters.insert("Never Fires", CardCounter::default());
        let (ranked, low_exposure) = rank_cards(&counters, 0.20, 100, 1000, &|_| "");
        assert!(ranked.is_empty());
        assert!(low_exposure.is_empty(), "a zero-games row must not be printed as noise -- it belongs in its own zero-exposure section");
    }

    #[test]
    fn rank_cards_sorts_the_ranked_list_by_enrichment_descending() {
        let mut counters = HashMap::new();
        counters.insert("Low", CardCounter { games: 25, failures: 5, round_sum: 250 }); // rate 0.20 -> 1.0x
        counters.insert("High", CardCounter { games: 25, failures: 20, round_sum: 250 }); // rate 0.80 -> 4.0x
        let (ranked, _) = rank_cards(&counters, 0.20, 100, 1000, &|_| "");
        assert_eq!(ranked[0].name, "High", "the higher-enrichment card must sort first");
        assert_eq!(ranked[1].name, "Low");
    }

    #[test]
    fn rank_cards_reports_round_without_as_the_complement_of_the_cards_own_round_sum() {
        // 100 games total, round_sum 1000 overall; this card is in 25 of
        // them summing to 250 rounds (mean 10) -- the OTHER 75 games must
        // then sum to 750 (mean 10 too, in this deliberately unconfounded
        // fixture), proving round_without is derived from the complement,
        // not from a second full pass over the corpus.
        let mut counters = HashMap::new();
        counters.insert("Card", CardCounter { games: 25, failures: 5, round_sum: 250 });
        let (ranked, _) = rank_cards(&counters, 0.20, 100, 1000, &|_| "");
        assert!((ranked[0].round_without - 10.0).abs() < 1e-9);
    }

    #[test]
    fn rank_cards_reports_round_without_as_nan_when_the_card_is_in_every_sampled_game() {
        // A basic Age A card with no "without" group at all -- must not
        // divide by zero or silently print zero as if it were a real mean.
        let mut counters = HashMap::new();
        counters.insert("Universal Card", CardCounter { games: 100, failures: 10, round_sum: 1000 });
        let (ranked, _) = rank_cards(&counters, 0.10, 100, 1000, &|_| "");
        assert!(ranked[0].round_without.is_nan());
    }

    #[test]
    fn rank_cards_attaches_the_group_a_card_cut_asks_for_via_its_lookup_closure() {
        let mut counters = HashMap::new();
        counters.insert("Anything", CardCounter { games: 25, failures: 5, round_sum: 250 });
        let (ranked, _) = rank_cards(&counters, 0.20, 100, 1000, &|_| "test-group");
        assert_eq!(ranked[0].group, "test-group");
    }

    #[test]
    fn move_kind_names_has_no_duplicates_and_has_the_full_variant_count() {
        let set: HashSet<&str> = ALL_MOVE_KIND_NAMES.iter().copied().collect();
        assert_eq!(set.len(), ALL_MOVE_KIND_NAMES.len(), "ALL_MOVE_KIND_NAMES must not repeat a name");
        // Every real Move constructed with dummy field values must map
        // back onto a name IN this list -- catches the list drifting out
        // of sync with move_kind_name's own match arms (see that const's
        // own doc: this is a tripwire, not a compiler guarantee).
        use tta::moves::{ChurchillChoice, PactSide};
        let samples = [
            Move::Take { slot: 0, cost: i32::MAX },
            Move::Build { card: CardId::NONE },
            Move::Develop { card: CardId::NONE },
            Move::Upgrade { from: CardId::NONE, to: CardId::NONE },
            Move::WonderStep { steps: 0 },
            Move::Pop,
            Move::PopFree,
            Move::Revolution { card: CardId::NONE },
            Move::PlayLeader { card: CardId::NONE },
            Move::PlayAction { card: CardId::NONE },
            Move::Destroy { card: CardId::NONE },
            Move::PlayTactic { card: CardId::NONE },
            Move::CopyTactic { card: CardId::NONE },
            Move::Aggression { card: CardId::NONE, target: 0 },
            Move::War { card: CardId::NONE, target: 0 },
            Move::OfferPact { card: CardId::NONE, target: 0, side: PactSide::Unspecified },
            Move::CancelPact { owner: 0 },
            Move::PrepareEvent { card: CardId::NONE },
            Move::RemoveLeaderYellow,
            Move::ColumbusColonize { card: CardId::NONE },
            Move::Barbarossa { card: CardId::NONE },
            Move::BachTheater { from: CardId::NONE, to: CardId::NONE },
            Move::TradeFoodAsResource,
            Move::TradeResourceAsFood,
            Move::Bid { n: 0 },
            Move::BidPass,
            Move::Defend { card: CardId::NONE },
            Move::DefendDone,
            Move::SendUnit { card: CardId::NONE },
            Move::SendBonus { card: CardId::NONE },
            Move::SendDiscard { card: CardId::NONE },
            Move::SendDone,
            Move::Choose { n: 0 },
            Move::Churchill { choice: ChurchillChoice::Culture },
            Move::EndTurn,
            Move::PolPass,
            Move::Resign,
        ];
        assert_eq!(samples.len(), ALL_MOVE_KIND_NAMES.len(), "this test's own sample list must cover every Move variant too");
        for mv in samples {
            assert!(set.contains(move_kind_name(mv)), "move_kind_name({mv:?}) = {:?} not in ALL_MOVE_KIND_NAMES", move_kind_name(mv));
        }
    }

    #[test]
    fn action_class_all_is_the_source_of_truth_this_file_pre_seeds_the_happenings_counters_from() {
        // Not much to assert beyond "it is non-empty and every label is
        // distinct" -- ActionClass::ALL's own exhaustiveness is checked in
        // corpus.rs, not duplicated here.
        let labels: HashSet<&str> = ActionClass::ALL.iter().map(|c| c.label()).collect();
        assert_eq!(labels.len(), ActionClass::ALL.len(), "ActionClass::ALL must not repeat a label");
    }

    #[test]
    fn card_group_classifies_a_wonder_and_a_government_into_distinct_named_groups() {
        assert_eq!(card_group(CardType::Wonder), "wonder");
        assert_eq!(card_group(CardType::Government), "government");
        assert_eq!(card_group(CardType::Event), "event");
        assert_ne!(card_group(CardType::Wonder), card_group(CardType::Government));
    }

    #[test]
    fn accumulate_counts_a_game_once_per_distinct_name_even_if_given_a_duplicate() {
        let mut counters = HashMap::new();
        // A HashSet upstream is what actually dedupes in `run`; this test
        // exercises `accumulate` directly with an already-deduped slice
        // (its own real contract) plus confirms two DIFFERENT games sum.
        accumulate(&mut counters, ["A", "B"], 10, true);
        accumulate(&mut counters, ["A"], 20, false);
        assert_eq!(counters["A"].games, 2);
        assert_eq!(counters["A"].failures, 1);
        assert_eq!(counters["A"].round_sum, 30);
        assert_eq!(counters["B"].games, 1);
    }
}
