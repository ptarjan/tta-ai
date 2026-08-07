//! `agreement` -- move-agreement analysis (`docs/REPLAY.md`, phase 1 of a
//! two-phase project; see that doc's own top section for what `replay.rs`'s
//! reconstruction already validates and does not).
//!
//! For each human decision point `bin/replay.rs`'s reconstruction reaches
//! (i.e. every point where `tta::replay_common::Replayer::try_apply` is
//! about to apply a real, journal-observed human `Move` -- see that module's
//! doc for exactly which call sites count and which are internal
//! auto-resolutions this file does not report on), asks a [`WeightedBot`]
//! (the same bot kind, loaded from the same champion weights, real league
//! play uses) to rank EVERY move in the identical legal-move list the human
//! faced -- BEFORE applying the human's real move -- and reports whether the
//! bot's #1 choice agreed, and if not, where the human's actual move fell in
//! the bot's own preference order. The bot's opinion is observation-only: the
//! game always continues down the human's REAL trajectory afterward, exactly
//! as `replay.rs` already does.
//!
//! This binary adds NO new reconstruction logic -- it is a thin consumer of
//! [`tta::replay_common::replay_game`]'s `record_decisions: true` path (the
//! same machinery `bin/replay.rs` itself calls with `false`), plus
//! [`WeightedBot::rank_moves`] (`bots/weighted/eval.rs`), a full-ranking
//! sibling of the `WeightedBot::choose` real gameplay already uses -- both
//! reuse the exact same trial-and-evaluate loop, so "the bot's opinion" can
//! never silently diverge between the two.
//!
//! # Legality (public row/board vs. private hands)
//!
//! `WeightedBot::rank_moves`/`evaluate` read the SAME `GameState`
//! `legal_moves` was computed against and that the real, live bot already
//! plays with -- the card row, played cards, boards, and discards are public
//! and already fair game for that evaluator; rivals' hands and unshuffled
//! deck order are private and `evaluate` never reads them (this is not new
//! plumbing introduced here -- it is the same evaluator real games already
//! trust). This binary does not bypass or extend that: it hands the bot
//! nothing beyond what `d.state`/`d.legal_moves` already carry, both of which
//! `replay.rs`'s own reconstruction already computed through the real engine.
//!
//! # Discard caveat
//!
//! Every discard in the BGO corpus is an ARBITRARY choice among valid
//! candidates, never a deduction (BGO never names a military card at draw
//! time -- `docs/REPLAY.md`'s `discard_solver` section). A decision point
//! reached after this game has already resolved one such arbitrary discard
//! is grounded in a guess, not a fact -- flagged per-line via
//! `discard_tainted`, computed once and cheaply from `Decision::
//! after_arbitrary_discard`. No other change is made on account of this: the
//! decision point is still reported, just labelled, so a later pass can
//! filter or weight it.
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
//! cargo run --profile difftest --bin agreement -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals experiments \
//!     <game_id> [game_id ...] > agreement.tsv
//! ```
//!
//! `experiments` is the directory holding `rust_champion_{2,3,4}p.json`
//! (gitignored champion weights -- see `docs/REPLAY.md`/this project's setup
//! notes for how to populate it; NOT committed to this repo).
//!
//! # Output format
//!
//! One tab-separated line per decision point on stdout (a per-game one-line
//! progress summary goes to stderr, so redirecting stdout alone gives a
//! clean data file); no header row, no dependency (this crate's
//! `[dependencies]` stays empty -- see `Cargo.toml`), matching the flat,
//! hand-rolled TSV shape `sources/bgo/index.tsv`/journals already use.
//! Columns, in order:
//!
//! 1. `game_id`
//! 2. `tier` (BGO's own skill ladder for this game -- `Prince`/`King`/
//!    `Warlord`/`Emperor`, `index.tsv`'s `level` column, already parsed by
//!    `corpus::GameMeta::tier`; NOT a proxy -- no stand-in like score or
//!    game length is used when this column is genuinely absent, see
//!    `docs/AGREEMENT.md`)
//! 3. `players` (2/3/4)
//! 4. `age` (`A`/`I`/`II`/`III`/`IV` -- `GameState::age_civil` at the
//!    decision point, the engine's own structural age tracker, not a
//!    re-parse of the journal's own age column)
//! 5. `round` (`GameState::round`)
//! 6. `lineno` (the journal line this decision was translated from -- go
//!    back to `sources/bgo/journals/<game_id>.tsv` with this to see the raw
//!    human text, or feed `game_id`/this to a future "print the board at
//!    this point" tool built on the same `Decision::state` snapshot)
//! 7. `category` (see [`Category`] below)
//! 8. `agreed` (`true`/`false` -- did the bot's own #1 ranked move equal the
//!    human's)
//! 9. `human_rank` (1-indexed position of the human's actual move in the
//!    bot's own ranked order, or the literal text `uncounted` -- see
//!    [`Category`]'s sibling note: this should never actually print
//!    `uncounted` in practice, since `human_move` is always drawn from
//!    `legal_moves` by construction, but the column exists per this
//!    project's own instruction rather than silently asserting instead)
//! 10. `legal_count` (size of the legal-move list at this point -- context
//!     for how meaningful "rank 3 of 4" vs "rank 3 of 40" is)
//! 11. `discard_tainted` (`true`/`false` -- see the module doc's "Discard
//!     caveat")
//! 12. `human_move` (`{:?}` of the `Move` the human actually played)
//! 13. `bot_top_move` (`{:?}` of the bot's own #1 ranked move)
//! 14. `human_score` (the bot's own `evaluate` score for the human's move)
//! 15. `bot_top_score` (the bot's own `evaluate` score for its #1 move)
//!
//! Example line (fields separated by real tabs, shown here with visible gaps
//! for readability):
//!
//! ```text
//! 7523818  Emperor  2  I  7  107  bid  false  3  5  false  Bid { n: 3 }  BidPass  12.04  14.87
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use tta::bots::weighted::eval::{load_weights, WeightedBot};
use tta::corpus::{self, GameMeta};
use tta::replay_common::{build_card_index, replay_game, Decision};
use tta::state::{ChoiceKind, Pending};
use tta::Move;

// ---------------------------------------------------------------------
// Move category
// ---------------------------------------------------------------------

/// The nine buckets this project's phase-1 brief names, plus `Other` for
/// every `Move` variant that doesn't fit one cleanly. Deliberately an
/// exhaustive match over `Move` (no wildcard arm) in [`categorize`]: a new
/// `Move` variant must be placed here explicitly rather than silently
/// falling into `Other`.
///
/// `Build` covers `Move::Build` AND `Move::Develop`/`Move::Upgrade` --
/// revised from phase 1's original mapping (which put `Develop`/`Upgrade` in
/// `Other`) after the phase-2 brief's own review: developing a technology or
/// upgrading a farm/mine/unit is a building action under TTA's rules (§3,
/// §4), not a structurally different decision, so folding them in gives
/// `build` its true volume instead of splintering it across three buckets.
///
/// `Bid` is a first-class category (also new in phase 2, split out of
/// `Other`) specifically BECAUSE `docs/HUMAN_PLAY.md`'s census flags "4p bot
/// barely contests colonization auctions" as a live, named finding -- that
/// claim can only be tested at the move level (does the bot's own ranking
/// actually disfavour bidding, or does something else block it) if `Bid`/
/// `BidPass` decisions are visible as their own bucket rather than buried in
/// `Other` alongside unrelated things.
///
/// What actually lands in `Other`, given `replay.rs`'s current coverage
/// (see its own module doc for what it does and does not translate): a
/// leader's own destroy/disband effect (`Destroy`), playing a plain action
/// card (`PlayAction` -- distinct from the `FreeCivil`-granting `PlayAction`
/// that immediately precedes a `Build`, which is still recorded as its own
/// decision point, separately, and separately bucketed as `Build`), and
/// three narrow `Pending::Choice` resolutions folded into one journal line
/// (`FreeBuild`/`FreeCivil`/`FoodOrRes`/`DestroyOwn` -- see
/// [`categorize_choice`]). None of these map cleanly onto this brief's named
/// buckets; none is currently reached via any OTHER `ChoiceKind` either
/// (`GainBlock`/`DiscardMilitary`/`Raid`/`Annex`/`Infiltrate`/`TakeRow`/
/// `WarTech`/`PlunderSplit` are all resolved by `replay_common.rs` via
/// `apply::apply` directly, never through the recorded `try_apply` path --
/// see that module's doc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    TakeCard,
    Build,
    IncreasePopulation,
    LeaderOrWonderStep,
    PoliticalAction,
    AggressionOrWar,
    Pact,
    Tactics,
    Bid,
    EndTurn,
    Other,
}

impl Category {
    fn name(self) -> &'static str {
        match self {
            Category::TakeCard => "take_card",
            Category::Build => "build",
            Category::IncreasePopulation => "increase_population",
            Category::LeaderOrWonderStep => "leader_or_wonder_step",
            Category::PoliticalAction => "political_action",
            Category::AggressionOrWar => "aggression_or_war",
            Category::Pact => "pact",
            Category::Tactics => "tactics",
            Category::Bid => "bid",
            Category::EndTurn => "end_turn",
            Category::Other => "other",
        }
    }
}

/// `Move::Choose` is a positional index into whatever `Pending::Choice` is
/// currently open -- its OWN variant carries no hint of what kind of
/// decision it resolves, so this reads `pending.top()` on the PRE-move state
/// `Decision::state` snapshots (exactly what was open at record time, before
/// the human's `Choose` cleared it) to recover that context. `PactOffer` is
/// the only `ChoiceKind` `replay_common.rs` currently records a `Choose`
/// decision point for AND that maps onto one of this brief's named buckets
/// (accepting a pact); every other reachable kind falls to `Other` -- see
/// [`Category`]'s own doc comment for the full list and why none of them
/// currently exist as recorded decision points anyway.
fn categorize_choice(pending: Option<&Pending>) -> Category {
    match pending {
        Some(Pending::Choice(c)) => match c.kind {
            ChoiceKind::PactOffer { .. } => Category::Pact,
            ChoiceKind::GainBlock
            | ChoiceKind::FreeCivil { .. }
            | ChoiceKind::FoodOrRes { .. }
            | ChoiceKind::FreeBuild
            | ChoiceKind::DestroyOwn
            | ChoiceKind::LosePop
            | ChoiceKind::LoseColony
            | ChoiceKind::FlipWonder
            | ChoiceKind::DiscardMilitary
            | ChoiceKind::Raid { .. }
            | ChoiceKind::Annex { .. }
            | ChoiceKind::Infiltrate { .. }
            | ChoiceKind::TakeRow { .. }
            | ChoiceKind::WarTech { .. }
            | ChoiceKind::PlunderSplit { .. } => Category::Other,
        },
        // A recorded `Move::Choose` always has an open `Pending::Choice` at
        // record time (`try_apply` only records after its own legality check
        // passes, and `Choose` is never legal without one) -- this arm is
        // unreachable in practice, kept only as a defensive fallback rather
        // than a `panic!`, matching this file's own "never crash on a
        // surprising real game" posture.
        _ => Category::Other,
    }
}

fn categorize(pre_move_pending: Option<&Pending>, mv: Move) -> Category {
    use Move::*;
    match mv {
        Take { .. } => Category::TakeCard,
        Build { .. } | Develop { .. } | Upgrade { .. } => Category::Build,
        Pop | PopFree => Category::IncreasePopulation,
        PlayLeader { .. } | WonderStep { .. } => Category::LeaderOrWonderStep,
        Revolution { .. } | PolPass => Category::PoliticalAction,
        War { .. } | Aggression { .. } => Category::AggressionOrWar,
        OfferPact { .. } => Category::Pact,
        PlayTactic { .. } | CopyTactic { .. } => Category::Tactics,
        Bid { .. } | BidPass => Category::Bid,
        EndTurn => Category::EndTurn,
        Choose { .. } => categorize_choice(pre_move_pending),
        Destroy { .. }
        | PlayAction { .. }
        | CancelPact { .. }
        | PrepareEvent { .. }
        | RemoveLeaderYellow
        | ColumbusColonize { .. }
        | Barbarossa { .. }
        | BachTheater { .. }
        | Defend { .. }
        | DefendDone
        | SendUnit { .. }
        | SendBonus { .. }
        | SendDiscard { .. }
        | SendDone
        | Churchill { .. }
        | Resign
        | TradeFoodAsResource
        | TradeResourceAsFood => Category::Other,
    }
}

// ---------------------------------------------------------------------
// TSV output
// ---------------------------------------------------------------------

/// Ranks `d`'s legal-move list with `bot`, prints one TSV line, and returns
/// whether the bot's own #1 choice agreed with the human's -- returned
/// rather than recomputed by the caller, so `bot.rank_moves` (a full 1-ply
/// search over every candidate) runs exactly once per decision point, not
/// twice.
fn print_decision(game_id: &str, tier: &str, d: &Decision, bot: &WeightedBot) -> bool {
    let ranked = bot.rank_moves(&d.state, &d.legal_moves);
    let category = categorize(d.state.pending.top(), d.human_move);
    let human_rank = ranked.iter().position(|&(mv, _)| mv == d.human_move);
    let agreed = human_rank == Some(0);
    let human_rank_field = match human_rank {
        Some(r) => (r + 1).to_string(),
        None => "uncounted".to_string(),
    };
    let human_score = ranked
        .iter()
        .find(|&&(mv, _)| mv == d.human_move)
        .map(|&(_, s)| s);
    let (bot_top_move, bot_top_score) = match ranked.first() {
        Some(&(mv, s)) => (format!("{mv:?}"), s),
        // `d.legal_moves` is never empty (a real game's `legal_moves()`
        // never returns one) -- defensive text rather than a panic.
        None => ("none".to_string(), f64::NAN),
    };
    println!(
        "{game_id}\t{tier}\t{}\t{:?}\t{}\t{}\t{}\t{agreed}\t{human_rank_field}\t{}\t{}\t{:?}\t{bot_top_move}\t{}\t{bot_top_score}",
        d.state.num_players,
        d.state.age_civil,
        d.state.round,
        d.lineno,
        category.name(),
        d.legal_moves.len(),
        d.after_arbitrary_discard,
        d.human_move,
        human_score.map(|s| s.to_string()).unwrap_or_else(|| "uncounted".to_string()),
    );
    agreed
}

// ---------------------------------------------------------------------
// PlanBot secondary comparison (top-1 only -- see `run_planbot`'s doc)
// ---------------------------------------------------------------------

/// Same top-1 question as [`print_decision`], answered by [`tta::bots::plan`]
/// (beam search over whole-turn sequences) instead of [`WeightedBot`]. This
/// is deliberately a SEPARATE, narrower path rather than a mode of
/// [`print_decision`]: `plan::pick` returns one move, not a priced ranking
/// of the whole legal-move list (it beam-searches SEQUENCES, not individual
/// candidates), so `human_rank`/`bot_top_score` have no equivalent here --
/// forcing them into the same TSV shape would print columns that are
/// meaningless for this bot. Output is a strict subset of `print_decision`'s
/// columns, same order, so a consumer that only reads the shared prefix
/// (`game_id, tier, players, age, round, lineno, category, agreed,
/// discard_tainted`) can treat both interchangeably.
fn print_decision_planbot(
    game_id: &str,
    tier: &str,
    d: &Decision,
    cfg: &tta::bots::plan::PlanConfig,
    stats: &mut tta::bots::plan::Stats,
    counters: &mut tta::bots::pending::Counters,
    rng: &mut tta::rng::PyRandom,
) -> bool {
    let category = categorize(d.state.pending.top(), d.human_move);
    let bot_move = tta::bots::plan::pick(cfg, stats, counters, rng, &d.state, &d.legal_moves);
    let agreed = bot_move == d.human_move;
    println!(
        "{game_id}\t{tier}\t{}\t{:?}\t{}\t{}\t{}\t{agreed}\t{}\t{}\t{:?}\t{bot_move:?}",
        d.state.num_players,
        d.state.age_civil,
        d.state.round,
        d.lineno,
        category.name(),
        d.legal_moves.len(),
        d.after_arbitrary_discard,
        d.human_move,
    );
    agreed
}

/// A cheap secondary check (`docs/AGREEMENT.md`'s "does search close the
/// gap" question): does beam search over whole-turn sequences agree with
/// humans more often than `WeightedBot`'s single-move ranking does? Meant
/// for a small subsample (10-20 games) -- `PlanConfig::default()`'s node
/// budget makes this meaningfully slower per decision than `rank_moves`,
/// which is a flat evaluate-and-sort over an already-known list.
fn run_planbot(index_path: &str, journals_dir: &str, weights_dir: &str, ids: &[String]) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    let mut weights_by_players: HashMap<u8, tta::bots::weighted::weights::Weights> = HashMap::new();
    for players in [2u8, 3, 4] {
        let path = Path::new(weights_dir).join(format!("rust_champion_{players}p.json"));
        weights_by_players.insert(players, load_weights(&path)?);
    }

    for id in ids {
        let Some(meta) = by_id.get(id.as_str()) else {
            eprintln!("{id}: not found in index.tsv");
            continue;
        };
        let path = format!("{journals_dir}/{id}.tsv");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{id}: no journal file ({e})");
                continue;
            }
        };
        let weights = *weights_by_players
            .get(&meta.players)
            .ok_or_else(|| format!("no champion weights loaded for {}p", meta.players))?;
        // Same `width: 2` override real league play uses for `BotKind::Plan`
        // (`bots/greedy.rs`'s `build_bots`) -- otherwise this would be
        // comparing a differently-tuned search than the one actually
        // fielded.
        let cfg = tta::bots::plan::PlanConfig { width: 2, weights, ..tta::bots::plan::PlanConfig::default() };
        let mut stats = tta::bots::plan::Stats::default();
        let mut counters = tta::bots::pending::Counters::default();
        // Seeded, not zero: a `PyRandom` stream this bot owns end-to-end for
        // the game, exactly as `build_bots`' per-seat rng is -- reusing one
        // fixed seed across every game would make every game's determinizer
        // draw the identical sequence.
        let mut rng = tta::rng::PyRandom::new(meta.id.parse::<i128>().unwrap_or(0));

        let result = replay_game(meta, &text, &card_index, true);
        let n = result.decisions.len();
        let mut agreed_n = 0usize;
        for d in &result.decisions {
            if print_decision_planbot(&meta.id, meta.tier.as_str(), d, &cfg, &mut stats, &mut counters, &mut rng) {
                agreed_n += 1;
            }
        }
        let status = if result.completed { "COMPLETE" } else { "STOPPED" };
        eprintln!(
            "{} [{}p] {status} after {} actions -- {n} decision points recorded, {agreed_n}/{n} top-1 agreement (PlanBot)",
            meta.id, meta.players, result.actions_consumed
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------
// main
// ---------------------------------------------------------------------

fn bot_for(bots: &HashMap<u8, WeightedBot>, players: u8) -> Result<&WeightedBot, String> {
    bots.get(&players)
        .ok_or_else(|| format!("no champion weights loaded for {players}p"))
}

fn run(index_path: &str, journals_dir: &str, weights_dir: &str, ids: &[String]) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    let mut bots: HashMap<u8, WeightedBot> = HashMap::new();
    for players in [2u8, 3, 4] {
        let path = Path::new(weights_dir).join(format!("rust_champion_{players}p.json"));
        let weights = load_weights(&path)?;
        bots.insert(players, WeightedBot::new(weights));
    }

    for id in ids {
        let Some(meta) = by_id.get(id.as_str()) else {
            eprintln!("{id}: not found in index.tsv");
            continue;
        };
        let path = format!("{journals_dir}/{id}.tsv");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{id}: no journal file ({e})");
                continue;
            }
        };
        let bot = bot_for(&bots, meta.players)?;
        let result = replay_game(meta, &text, &card_index, true);
        let n = result.decisions.len();
        let mut agreed_n = 0usize;
        for d in &result.decisions {
            if print_decision(&meta.id, meta.tier.as_str(), d, bot) {
                agreed_n += 1;
            }
        }
        let status = if result.completed { "COMPLETE" } else { "STOPPED" };
        eprintln!(
            "{} [{}p] {status} after {} actions -- {n} decision points recorded, {agreed_n}/{n} top-1 agreement",
            meta.id, meta.players, result.actions_consumed
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut argv: Vec<String> = env::args().skip(1).collect();
    // `--planbot`: secondary comparison against `bots::plan` instead of
    // `WeightedBot` -- see `run_planbot`'s doc. A leading flag, not a
    // separate binary, so both modes share `main`'s own arg validation and
    // every other file in this crate need not learn a second binary name.
    let planbot = !argv.is_empty() && argv[0] == "--planbot";
    if planbot {
        argv.remove(0);
    }
    if argv.len() < 4 {
        eprintln!("usage: agreement [--planbot] <index.tsv> <journals_dir> <weights_dir> <game_id> [game_id ...]");
        return ExitCode::FAILURE;
    }
    let result = if planbot {
        run_planbot(&argv[0], &argv[1], &argv[2], &argv[3..])
    } else {
        run(&argv[0], &argv[1], &argv[2], &argv[3..])
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tta::cards::CardId;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).expect("known card")
    }

    #[test]
    fn categorize_maps_take_build_pop_leader_wonder_step_and_end_turn() {
        assert_eq!(categorize(None, Move::Take { slot: 0 }), Category::TakeCard);
        assert_eq!(categorize(None, Move::Build { card: card("Bronze") }), Category::Build);
        assert_eq!(categorize(None, Move::Pop), Category::IncreasePopulation);
        assert_eq!(categorize(None, Move::PopFree), Category::IncreasePopulation);
        assert_eq!(categorize(None, Move::PlayLeader { card: card("Hammurabi") }), Category::LeaderOrWonderStep);
        assert_eq!(categorize(None, Move::WonderStep { steps: 1 }), Category::LeaderOrWonderStep);
        assert_eq!(categorize(None, Move::EndTurn), Category::EndTurn);
    }

    #[test]
    fn categorize_maps_political_aggression_war_pact_and_tactics() {
        assert_eq!(categorize(None, Move::Revolution { card: card("Despotism") }), Category::PoliticalAction);
        assert_eq!(categorize(None, Move::PolPass), Category::PoliticalAction);
        assert_eq!(categorize(None, Move::War { card: card("Warriors"), target: 1 }), Category::AggressionOrWar);
        assert_eq!(categorize(None, Move::Aggression { card: card("Warriors"), target: 1 }), Category::AggressionOrWar);
        assert_eq!(
            categorize(None, Move::OfferPact { card: card("Warriors"), target: 1, side: tta::moves::PactSide::Unspecified }),
            Category::Pact
        );
        assert_eq!(categorize(None, Move::PlayTactic { card: card("Warriors") }), Category::Tactics);
        assert_eq!(categorize(None, Move::CopyTactic { card: card("Warriors") }), Category::Tactics);
    }

    #[test]
    fn categorize_buckets_develop_upgrade_as_build_bid_as_bid_and_the_rest_as_other() {
        // Revised in phase 2: Develop/Upgrade are building actions under
        // TTA's rules, and Bid/BidPass get their own bucket so the
        // colonization-auction census finding is testable at the move
        // level -- see the module doc above `Category` for the rationale.
        assert_eq!(categorize(None, Move::Develop { card: card("Bronze") }), Category::Build);
        assert_eq!(categorize(None, Move::Upgrade { from: card("Bronze"), to: card("Iron") }), Category::Build);
        assert_eq!(categorize(None, Move::Bid { n: 3 }), Category::Bid);
        assert_eq!(categorize(None, Move::BidPass), Category::Bid);
        assert_eq!(categorize(None, Move::Destroy { card: card("Bronze") }), Category::Other);
        assert_eq!(categorize(None, Move::PlayAction { card: card("Reserves (I)") }), Category::Other);
    }

    /// A `Move::Choose` resolving an open `PactOffer` pending is bucketed as
    /// `Pact` (accepting/refusing a proposed pact), using the PRE-move
    /// pending snapshot -- `Choose` itself carries no context of its own.
    #[test]
    fn categorize_reads_a_choose_moves_category_off_the_pre_move_pending() {
        let pact_pending = Pending::Choice(tta::state::Choice {
            player: 0,
            kind: ChoiceKind::PactOffer { owner: 1, card: card("Warriors"), a: 0, b: 1 },
            options: Default::default(),
        });
        assert_eq!(categorize(Some(&pact_pending), Move::Choose { n: 0 }), Category::Pact);

        let free_build_pending = Pending::Choice(tta::state::Choice {
            player: 0,
            kind: ChoiceKind::FreeBuild,
            options: Default::default(),
        });
        assert_eq!(categorize(Some(&free_build_pending), Move::Choose { n: 0 }), Category::Other);

        // No pending open at all (should not happen for a real recorded
        // decision, but must not panic either) -- defensively `Other`.
        assert_eq!(categorize(None, Move::Choose { n: 0 }), Category::Other);
    }
}
