//! `whydiverge` -- WHY the champion bot's top move diverges from the
//! human's real move at `take_card`/`increase_population` decision points
//! (`analysis/divergence_raw_2026-08-25.txt`'s two highest-disagreement
//! categories). `agreement.rs`/`multcheck.rs` already answer WHETHER the bot
//! agreed and WHETHER a key ever moves an argmax; this binary answers which
//! [`tta::bots::weighted::weights::WeightKey`] carried a SPECIFIC observed
//! disagreement.
//!
//! The decomposition is exact, not estimated: `eval::candidate_features`
//! returns the same per-`WeightKey` linear vector `evaluate`/`rank_moves`
//! dot against `w` (pinned by that module's own
//! `linear_features_dotted_with_a_weight_vector_reproduces_evaluate_exactly`
//! test), so for the bot's own top-scoring candidate B and the human's real
//! move H, `score(B) - score(H) = sum_k w[k] * (phi_B[k] - phi_H[k])`
//! exactly -- each key's term is its own signed contribution to the bot
//! preferring B over H.
//!
//! ```text
//! cargo run --release --bin whydiverge -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals experiments \
//!     <game_id> [game_id ...] > whydiverge.tsv
//! ```
//!
//! # Output
//!
//! One TSV line per `(players, category, key)` on stdout, sorted by
//! `n_top_key` descending (a per-game one-line progress summary goes to
//! stderr, matching `agreement.rs`'s split). No header row -- same
//! convention. Columns: `players`, `category` (`take_card`/
//! `increase_population`), `key` (`WeightKey` `Debug` name, resolved at
//! runtime -- never a literal, per this repo's
//! `every_weight_key_is_named_by_production_source_outside_its_own_declaration`
//! rule), `n_decisions` (how many analysed decisions had a nonzero term for
//! this key), `sum_term` (signed sum of `w[k] * (phi_B[k] - phi_H[k])` over
//! those decisions), `sum_abs_term`, `n_top_key` (how many decisions this
//! key held the single largest POSITIVE term of any key -- the main reason
//! the bot preferred its own move).
//!
//! # Scope
//!
//! Only decisions where `categorize` reports `TakeCard`/`IncreasePopulation`
//! AND the bot's own top-scoring candidate (by the SAME `candidate_features`
//! vector this file decomposes, not a second `rank_moves` call) is NOT the
//! human's move -- an agreeing decision has nothing to explain. Among those,
//! a decision whose human move is absent from the candidate list (BGO
//! occasionally logs a move `legal_moves` does not itself produce -- a known,
//! separate ~48-row corpus finding, not this file's to fix) is counted and
//! skipped, never silently dropped.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tta::bots::weighted::eval::{candidate_features, dot, load_weights};
use tta::bots::weighted::weights::WeightKey;
use tta::corpus::{self, GameMeta};
use tta::replay_common::{build_card_index, categorize, replay_game, Category};

/// Running totals for one `(players, category, key)` bucket.
#[derive(Default, Clone, Copy)]
struct Agg {
    n_decisions: u64,
    sum_term: f64,
    sum_abs_term: f64,
    n_top_key: u64,
}

/// First-candidate-wins strict-max index, matching
/// [`tta::bots::weighted::eval::WeightedBot::choose`]'s own tie-break
/// (`multcheck.rs`'s `argmax` does the identical thing for the identical
/// reason -- kept as a private copy here rather than a shared helper since
/// neither binary exposes one).
fn argmax(scores: &[f64]) -> usize {
    let mut best = 0usize;
    for (i, &v) in scores.iter().enumerate().skip(1) {
        if v > scores[best] {
            best = i;
        }
    }
    best
}

/// True when `category` is one of the two this project's WHY pass targets.
fn in_scope(category: Category) -> bool {
    match category {
        Category::TakeCard | Category::IncreasePopulation => true,
        Category::Build
        | Category::LeaderOrWonderStep
        | Category::PoliticalAction
        | Category::AggressionOrWar
        | Category::Pact
        | Category::Tactics
        | Category::Bid
        | Category::EndTurn
        | Category::Other => false,
    }
}

/// Per-game counters, reported on stderr -- mirrors `agreement.rs`'s own
/// per-game progress line shape.
#[derive(Default)]
struct GameCounts {
    in_scope: u64,
    agreed: u64,
    analysed: u64,
    skipped_absent: u64,
}

fn run(index_path: &str, journals_dir: &str, weights_dir: &str, ids: &[String]) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    let mut weights_by_players: HashMap<u8, tta::bots::weighted::weights::Weights> = HashMap::new();
    for players in [2u8, 3, 4] {
        let path = Path::new(weights_dir).join(format!("rust_champion_{players}p.json"));
        weights_by_players.insert(players, load_weights(&path)?);
    }

    // `(players, category name, key)` -> aggregate. `category.name()` (a
    // `&'static str`) stands in for `Category` itself as a map key, since
    // `Category` (declared in `replay_common.rs`, not editable here) derives
    // `PartialEq`/`Eq` but not `Hash`.
    let mut agg: HashMap<(u8, &'static str, WeightKey), Agg> = HashMap::new();
    let mut total_analysed = 0u64;
    let mut total_skipped_absent = 0u64;
    let mut total_agreed = 0u64;
    let mut total_in_scope = 0u64;
    let mut games_run = 0u64;
    let mut games_failed = 0u64;

    for id in ids {
        let Some(meta) = by_id.get(id.as_str()) else {
            eprintln!("{id}: not found in index.tsv");
            games_failed += 1;
            continue;
        };
        let path = format!("{journals_dir}/{id}.tsv");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{id}: no journal file ({e})");
                games_failed += 1;
                continue;
            }
        };
        let weights = *weights_by_players
            .get(&meta.players)
            .ok_or_else(|| format!("no champion weights loaded for {}p", meta.players))?;

        let result = replay_game(meta, &text, &card_index, true);
        let n = result.decisions.len();
        let mut gc = GameCounts::default();

        for d in &result.decisions {
            let category = categorize(d.state.pending.top(), d.human_move);
            if !in_scope(category) {
                continue;
            }
            gc.in_scope += 1;

            let cf = candidate_features(&d.state, &d.legal_moves, false, &weights);
            let scores: Vec<f64> = cf.iter().map(|(_, f)| dot(&weights, f)).collect();
            let bot_top_idx = argmax(&scores);
            let bot_top_move = cf[bot_top_idx].0;

            if bot_top_move == d.human_move {
                gc.agreed += 1;
                continue;
            }

            let Some(human_idx) = cf.iter().position(|&(mv, _)| mv == d.human_move) else {
                gc.skipped_absent += 1;
                continue;
            };

            gc.analysed += 1;
            let phi_b = &cf[bot_top_idx].1;
            let phi_h = &cf[human_idx].1;

            let mut top_key: Option<(WeightKey, f64)> = None;
            for (i, &k) in WeightKey::ALL.iter().enumerate() {
                let term = weights.get(k) * (phi_b[i] - phi_h[i]);
                if term != 0.0 {
                    let entry = agg.entry((meta.players, category.name(), k)).or_default();
                    entry.n_decisions += 1;
                    entry.sum_term += term;
                    entry.sum_abs_term += term.abs();
                }
                let is_new_top = match top_key {
                    Some((_, best)) => term > best,
                    None => true,
                };
                if is_new_top {
                    top_key = Some((k, term));
                }
            }
            if let Some((k, best)) = top_key {
                if best > 0.0 {
                    agg.entry((meta.players, category.name(), k)).or_default().n_top_key += 1;
                }
            }
        }

        total_in_scope += gc.in_scope;
        total_agreed += gc.agreed;
        total_analysed += gc.analysed;
        total_skipped_absent += gc.skipped_absent;
        games_run += 1;

        let status = if result.completed { "COMPLETE" } else { "STOPPED" };
        eprintln!(
            "{} [{}p] {status} after {} actions -- {n} decision points, {} take_card/increase_population, \
             {} agreed, {} analysed, {} skipped (human move absent)",
            meta.id, meta.players, result.actions_consumed, gc.in_scope, gc.agreed, gc.analysed, gc.skipped_absent
        );
    }

    let mut rows: Vec<((u8, &'static str, WeightKey), Agg)> = agg.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1.n_top_key));

    for ((players, category, key), a) in &rows {
        println!(
            "{players}\t{category}\t{key:?}\t{}\t{}\t{}\t{}",
            a.n_decisions, a.sum_term, a.sum_abs_term, a.n_top_key
        );
    }

    eprintln!(
        "whydiverge: {games_run} games run, {games_failed} failed, {total_in_scope} take_card/increase_population \
         decisions, {total_agreed} agreed (skipped), {total_analysed} analysed, {total_skipped_absent} skipped \
         (human move absent)"
    );

    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 4 {
        eprintln!("usage: whydiverge <index.tsv> <journals_dir> <weights_dir> <game_id> [game_id ...]");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1], &argv[2], &argv[3..]) {
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
    fn argmax_picks_first_on_tie() {
        assert_eq!(argmax(&[1.0, 2.0, 2.0, 0.5]), 1);
    }

    #[test]
    fn argmax_picks_the_strict_max() {
        assert_eq!(argmax(&[1.0, 5.0, 2.0]), 1);
    }

    #[test]
    fn in_scope_accepts_only_take_card_and_increase_population() {
        assert!(in_scope(Category::TakeCard));
        assert!(in_scope(Category::IncreasePopulation));
        assert!(!in_scope(Category::Build));
        assert!(!in_scope(Category::EndTurn));
        assert!(!in_scope(Category::Other));
    }
}
