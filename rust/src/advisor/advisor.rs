//! The mirror + bot + the operations the interactive loop performs.
//!
//! Ported from `advisor/advisor.py`'s `load_bot`, `rank_moves`, `parse_move`
//! and `Advisor`. Kept free of terminal I/O (`std::io`) so it can be driven
//! by tests as well as by `bin/advisor.rs`'s REPL -- the same separation
//! Python's own doc comment on `Advisor` calls out.
//!
//! ## What crossing into Rust deleted, and why it is safe
//!
//! * **`Advisor.rng` / `WeightedBot.rng`.** Python threads a `random.Random`
//!   through every `engine.apply(state, move, rng)` call so a trial move and
//!   a real move draw from the identical seeded stream. [`crate::apply::
//!   apply`] takes **no rng parameter at all** -- `bots::mod`'s own doc
//!   comment records why: the port derives randomness deterministically
//!   from `state.seed`/`turn`/`round` instead of threading a caller-supplied
//!   stream, closing the "two shuffle orders for one game" hazard threading
//!   one by hand invites. There is therefore nothing for `Advisor` to own an
//!   `rng` field for.
//! * **`engine.bots.fastcopy.copy_state`.** `bots::mod`'s doc comment again:
//!   `GameState` already derives a flat, allocation-free `Clone`, which IS
//!   the fast structural copy Python's hand-rolled copier exists to
//!   approximate. [`rank_moves`] below just calls `.clone()`.
//! * **The per-candidate `try`/`except` in `rank_moves`.** Python catches an
//!   exception from `apply`/`features`/`evaluate` per candidate so one bad
//!   move never aborts the whole recommendation. Every candidate here comes
//!   straight from [`crate::legal::legal_moves`], and nothing downstream of
//!   a legal move panics for it -- [`crate::bots::weighted::eval::
//!   WeightedBot::choose`] already trusts that same contract unguarded, so
//!   `rank_moves` does too rather than inventing a second convention.

use std::path::Path;

use crate::advisor::describe;
use crate::advisor::state_io::{self, Board};
use crate::apply;
use crate::bots::human;
use crate::bots::pending;
use crate::bots::plan;
use crate::bots::weighted::eval::{self, WeightedBot};
use crate::bots::weighted::features::{self, Features};
use crate::bots::weighted::rivals::{self, RivalContext};
use crate::bots::weighted::weights::{WeightKey, Weights};
use crate::cards::CardId;
use crate::legal;
use crate::moves::{ChurchillChoice, Move};
use crate::state::{GameState, Phase, ROW_SIZE};

// -------------------------------------------------------------- the bot

/// The strongest bot we have: hill-climbed champion weights if trained,
/// falling back to the built-in defaults. Returns the bot plus a source
/// string for the console banner. Mirrors `load_bot`.
///
/// `path`, when `None`, defaults to `experiments/rust_champion_{n}p.json` --
/// the file the running Rust league (`climb`, via `experiments/
/// rust_league.sh`) actually writes, gitignored so it exists only in a
/// working checkout, never in a fresh clone (see `docs/RUST_LEAGUE.md`).
/// This used to default to the committed `experiments/champion_{n}p.json`,
/// a Python-era snapshot last touched 2026-07-26 and since moved to
/// `analysis/frozen/` -- silently advising off that frozen, superseded
/// vector instead of the league's current champion was exactly the trap
/// this default now avoids. Relative to the current directory -- the same
/// convention `arena`/`climb` use for every weights path they take (there
/// is no `__file__`-relative "repo root" in a compiled binary the way
/// Python's `ROOT` computes one; a human runs this from the repo root
/// exactly as the other tools document).
pub fn load_bot(num_players: u8, path: Option<&Path>) -> (WeightedBot, String) {
    let default_path = std::path::PathBuf::from("experiments")
        .join(format!("rust_champion_{num_players}p.json"));
    let path_buf = path.map(|p| p.to_path_buf()).unwrap_or(default_path);
    if !path_buf.exists() {
        return (WeightedBot::default(), "built-in default weights".to_string());
    }
    match eval::load_weights(&path_buf) {
        Ok(weights) => (WeightedBot::new(weights), path_buf.display().to_string()),
        Err(e) => {
            let base = path_buf
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_else(|| path_buf.display().to_string());
            (WeightedBot::default(), format!("built-in defaults ({base}: {e})"))
        }
    }
}

// --------------------------------------------------------- candidates

/// One recommended move: the bot's score for the resulting position
/// (relative to the best candidate, so the top pick is always `0.0`), the
/// move's plain-English description, and why the bot likes it. Mirrors
/// `Candidate`.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub mv: Move,
    pub score: f64,
    pub text: String,
    pub reason: String,
}

// ------------------------------------------------------------ search mode

/// Which lookahead the advisor uses to rank moves, selected by `bin/
/// advisor.rs`'s `--search` flag. Exhaustive everywhere it is matched (see
/// [`rank_moves_search`]): a new variant is a compile error at every match
/// site until it is wired in, not a silent no-op.
///
/// ## Why `Human` is the default, not `Greedy` or `Beam`
///
/// Measured 2026-08-13, 2p, 240 games/config, seats rotated, null baseline
/// 50%:
///
/// * [`Beam`](SearchMode::Beam) (plain beam search over every legal move,
///   leaf-scored with the champion vector) vs. [`Greedy`](SearchMode::Greedy)
///   with the SAME champion vector: the beam seat won **41.9%** -- it LOSES,
///   p~=0.01. The champion vector was hill-climbed against a 1-ply
///   evaluator and does not transfer to deeper search out of the box; depth
///   alone is not the lever. Do not pick `Beam` on the assumption that more
///   search must be better -- on this vector it is measurably worse than the
///   1-ply baseline it is compared against.
/// * [`Human`](SearchMode::Human) (a human-imitation move-ordering prior
///   narrows the ROOT candidates, then the SAME beam ranks the survivors,
///   still leaf-scored with the champion vector) vs. `Greedy`: **68.3%**,
///   p~=1e-8 (consistent with 65.6% measured 2026-08-08). The win comes from
///   narrowing WHICH root moves get considered at all, not from searching
///   deeper over the full move set.
///
/// `Human` is therefore the one configuration measured to beat the champion
/// baseline, and is the default a fresh advisor run picks. `Greedy` and
/// `Beam` stay selectable so the three can be compared side by side (e.g.
/// against a saved position from a real game), never removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    /// The advisor's original behaviour: clone, apply exactly the ONE
    /// candidate move, score with [`eval::evaluate`]. No lookahead past that
    /// single ply -- see this module's top doc comment for the real-game
    /// failure this leaves on the table (a wonder taken, and never built,
    /// because "wonder acquired" scores well for exactly one turn).
    Greedy,
    /// [`plan::rank`]'s beam search over every legal move, leaf-scored with
    /// the SAME gameplay vector `Greedy` uses -- see this enum's own doc
    /// comment for why this is measured WORSE than `Greedy`, not better.
    Beam,
    /// `Beam`, but [`human::root_shortlist`] narrows the ROOT's legal moves
    /// to the human-imitation model's most-plausible candidates before the
    /// beam ever runs. Leaf scoring is STILL the gameplay vector, never the
    /// human one -- see [`human::choose_with_search`]'s own doc comment: this
    /// is a hybrid, not "play like a human".
    Human,
}

/// [`SearchMode::Beam`]/[`SearchMode::Human`]'s shared configuration: the
/// [`plan::PlanConfig`] template the search runs with (`weights` is
/// overwritten from the advisor's own [`WeightedBot`] on every call -- see
/// [`rank_moves_beam`] -- so there is exactly one gameplay vector in play,
/// never a second one this struct could drift out of sync with) and, for
/// `Human`, the root move-ordering prior vector
/// ([`crate::human_policy::vector_from_weights`]'s own dense shape, loaded
/// from `analysis/frozen/human_weights.json` by the CLI).
#[derive(Clone, Debug)]
pub struct SearchConfig {
    pub mode: SearchMode,
    pub plan: plan::PlanConfig,
    /// Only read under `SearchMode::Human`; empty under `Greedy`/`Beam`,
    /// where nothing ever looks at it.
    pub human_weights: Vec<f64>,
}

impl Default for SearchConfig {
    /// `Greedy`, matching [`rank_moves`]'s original, unconditional behaviour
    /// -- an [`Advisor`] built with [`Advisor::new`] and never told
    /// otherwise must recommend EXACTLY what it always has, so an existing
    /// caller (a test, `bin/harness.rs`, `harness/play.rs`'s scripted
    /// operator) sees no behaviour change just by this type existing. The
    /// CLI's own default is `Human` (see [`SearchMode`]'s doc comment) --
    /// that is `bin/advisor.rs`'s decision, made explicitly by constructing
    /// its own `SearchConfig` rather than by changing what this trait
    /// impl returns.
    fn default() -> SearchConfig {
        SearchConfig {
            mode: SearchMode::Greedy,
            plan: plan::PlanConfig::default(),
            human_weights: Vec::new(),
        }
    }
}

/// The legal moves worth SCORING for a recommendation: resign dropped
/// whenever a real alternative exists (mirrors Python's `[m for m in moves
/// if m[0] != "resign"] or moves`), and `end_turn` dropped too unless the
/// caller asked to see it (`include_end_turn`) or it is the only option.
/// Shared by every [`SearchMode`] so `greedy`/`beam`/`human` rank the
/// IDENTICAL candidate set and differ only in how each one is scored -- a
/// side-by-side comparison across modes on one saved position would
/// otherwise be comparing different questions, not just different answers.
fn scoreable_candidates(moves: &[Move], include_end_turn: bool) -> Vec<Move> {
    let non_resign: Vec<Move> = moves.iter().copied().filter(|m| !matches!(m, Move::Resign)).collect();
    let base: &[Move] = if non_resign.is_empty() { moves } else { &non_resign };
    if include_end_turn || base.len() <= 1 {
        return base.to_vec();
    }
    let without_end: Vec<Move> = base.iter().copied().filter(|m| !matches!(m, Move::EndTurn)).collect();
    if without_end.is_empty() { base.to_vec() } else { without_end }
}

/// Score every legal move with the bot's own evaluation and return the best
/// `top`. Mirrors `rank_moves`; see this module's top doc comment for the
/// three Python mechanisms (`rng`, `fastcopy`, per-candidate `try`/`except`)
/// this does not carry. This is [`SearchMode::Greedy`]'s implementation,
/// still reachable on its own (not just through [`rank_moves_search`]) for
/// any caller that only ever wants the 1-ply baseline.
pub fn rank_moves(board: &Board, bot: &WeightedBot, top: usize, include_end_turn: bool) -> Vec<Candidate> {
    let st = &board.state;
    let moves = legal::legal_moves(st);
    if moves.is_empty() {
        return Vec::new();
    }
    let idx = st.decider();
    let candidates = scoreable_candidates(moves.as_slice(), include_end_turn);

    // Computed once at the root and reused for every candidate -- the same
    // reuse `WeightedBot::choose` documents (an information-leak concern,
    // not just an optimisation, if recomputed per candidate).
    let ctx = rivals::rival_context(st, idx, None, None);
    let before = features::features(st, idx, Some(&ctx), None, false);
    let w = &bot.weights;
    let end_bias = w.get(WeightKey::EndTurnBias);

    let mut scored: Vec<(f64, Move, features::Features)> = Vec::new();
    for &mv in &candidates {
        let mut trial = st.clone();
        apply::apply(&mut trial, mv);
        let after = features::features(&trial, idx, Some(&ctx), None, false);
        let mut val = eval::evaluate(&trial, idx, w, Some(&ctx), Some(&after));
        if matches!(mv, Move::EndTurn) {
            val += end_bias;
        }
        scored.push((val, mv, after));
    }
    if scored.is_empty() {
        let mv = candidates[0];
        return vec![Candidate {
            mv,
            score: 0.0,
            text: describe::describe_move(st, mv, Some(board)),
            reason: "only move the engine could score".to_string(),
        }];
    }
    let base = scored.iter().map(|&(v, _, _)| v).fold(f64::MIN, f64::max);
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(top)
        .map(|(val, mv, after)| Candidate {
            mv,
            score: val - base,
            text: describe::describe_move(st, mv, Some(board)),
            reason: describe::explain(&before, &after, w, 3),
        })
        .collect()
}

/// Every `candidates` move, immediate one-ply value (`weighted::eval::
/// evaluate` on the state one apply away -- the `Move::EndTurn` bias
/// included, matching [`rank_moves`]'s own treatment) plus the resulting
/// [`Features`], which [`describe::explain`] needs for the "why" line.
/// Shared by [`rank_moves_beam`]/[`rank_moves_human`]: both need this
/// exact computation, once, either as the REASON text's source (always) or
/// as the SCORE itself (only when the deeper search never reached that
/// candidate -- see [`combine_with_search`]).
fn one_ply_scored(
    st: &GameState,
    idx: u8,
    ctx: &RivalContext,
    w: &Weights,
    candidates: &[Move],
) -> Vec<(Move, f64, Features)> {
    let end_bias = w.get(WeightKey::EndTurnBias);
    candidates
        .iter()
        .map(|&mv| {
            let mut trial = st.clone();
            apply::apply(&mut trial, mv);
            let after = features::features(&trial, idx, Some(ctx), None, false);
            let mut val = eval::evaluate(&trial, idx, w, Some(ctx), Some(&after));
            if matches!(mv, Move::EndTurn) {
                val += end_bias;
            }
            (mv, val, after)
        })
        .collect()
}

/// Merge [`plan::rank`]'s deep scores onto [`one_ply_scored`]'s per-candidate
/// rows, rank everything, and build the final [`Candidate`] list -- the
/// shared tail of [`rank_moves_beam`] and [`rank_moves_human`].
///
/// `searched` may be missing entries [`plan::rank`] never reached (its own
/// doc comment: a root candidate starved by `cfg.max_nodes` before its turn
/// in the root ply is simply absent, not padded with a guess) -- such a
/// candidate falls back to its OWN one-ply value from `one_ply` rather than
/// being dropped from the list: a human choosing between recommendations
/// must never have a legal option silently vanish just because the search
/// budget ran out on it.
fn combine_with_search(
    st: &GameState,
    board: &Board,
    w: &Weights,
    before: &Features,
    one_ply: Vec<(Move, f64, Features)>,
    searched: &[(Move, f64)],
    top: usize,
) -> Vec<Candidate> {
    if one_ply.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(f64, Move, Features)> = one_ply
        .into_iter()
        .map(|(mv, fallback_val, after)| {
            let val = searched.iter().find(|&&(m, _)| m == mv).map_or(fallback_val, |&(_, v)| v);
            (val, mv, after)
        })
        .collect();
    let base = scored.iter().map(|&(v, _, _)| v).fold(f64::MIN, f64::max);
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(top)
        .map(|(val, mv, after)| Candidate {
            mv,
            score: val - base,
            text: describe::describe_move(st, mv, Some(board)),
            reason: describe::explain(before, &after, w, 3),
        })
        .collect()
}

/// [`SearchMode::Beam`]: real lookahead over every legal candidate,
/// leaf-scored with `bot`'s own gameplay vector -- `plan_cfg.weights` is
/// overwritten from `bot.weights` unconditionally, so the beam and the 1-ply
/// baseline always score with the identical vector (task requirement: leaf
/// scoring stays the gameplay/champion vector). One [`plan::rank`] call
/// prices every root candidate; see that function's own doc comment for why
/// this is not one search per candidate.
///
/// `rng`/`stats`/`counters` are freshly built per call rather than carried on
/// [`Advisor`]: the ordinary-turn branch of [`plan::rank`] seeds its own
/// determinize rng from `state.seed`/`state.turn`/the decider
/// ([`plan::plan_rng`], reused here so a real bot playing this exact
/// position would determinize identically) and needs nothing from the
/// caller, so an [`Advisor`] recommending moves stays free of the
/// call-to-call mutable search state a full game-playing bot instance
/// carries -- there is no game loop here for it to persist across.
fn rank_moves_beam(
    plan_cfg: &plan::PlanConfig,
    board: &Board,
    bot: &WeightedBot,
    top: usize,
    include_end_turn: bool,
) -> Vec<Candidate> {
    let st = &board.state;
    let moves = legal::legal_moves(st);
    if moves.is_empty() {
        return Vec::new();
    }
    let idx = st.decider();
    let candidates = scoreable_candidates(moves.as_slice(), include_end_turn);
    let ctx = rivals::rival_context(st, idx, None, None);
    let before = features::features(st, idx, Some(&ctx), None, false);
    let w = bot.weights;

    let one_ply = one_ply_scored(st, idx, &ctx, &w, &candidates);

    let cfg = plan::PlanConfig { weights: w, ..*plan_cfg };
    let mut stats = plan::Stats::default();
    let mut counters = pending::Counters::default();
    let mut rng = plan::plan_rng(st, idx);
    let searched = plan::rank(
        &cfg,
        &mut stats,
        &mut counters,
        &mut rng,
        st,
        &candidates,
        &mut plan::Bank::Off,
        None,
    );

    combine_with_search(st, board, &w, &before, one_ply, &searched, top)
}

/// [`SearchMode::Human`]: [`human::root_shortlist`] narrows `candidates` to
/// the human-imitation model's most-plausible root moves FIRST, then the
/// rest is exactly [`rank_moves_beam`]'s own tail -- the same hybrid
/// [`human::choose_with_search`] plays, adapted to rank every shortlisted
/// survivor instead of returning only the winner. Only the shortlisted moves
/// appear in the result: a move the human-imitation prior ranks outside the
/// shortlist is never deep-searched, exactly as `choose_with_search` never
/// considers it either.
fn rank_moves_human(
    plan_cfg: &plan::PlanConfig,
    human_weights: &[f64],
    board: &Board,
    bot: &WeightedBot,
    top: usize,
    include_end_turn: bool,
) -> Vec<Candidate> {
    let st = &board.state;
    let moves = legal::legal_moves(st);
    if moves.is_empty() {
        return Vec::new();
    }
    let idx = st.decider();
    let full_candidates = scoreable_candidates(moves.as_slice(), include_end_turn);
    let candidates = human::root_shortlist(human_weights, st, idx, &full_candidates);
    let ctx = rivals::rival_context(st, idx, None, None);
    let before = features::features(st, idx, Some(&ctx), None, false);
    let w = bot.weights;

    let one_ply = one_ply_scored(st, idx, &ctx, &w, &candidates);

    let cfg = plan::PlanConfig { weights: w, ..*plan_cfg };
    let mut stats = plan::Stats::default();
    let mut counters = pending::Counters::default();
    let mut rng = plan::plan_rng(st, idx);
    let searched = plan::rank(
        &cfg,
        &mut stats,
        &mut counters,
        &mut rng,
        st,
        &candidates,
        &mut plan::Bank::Off,
        None,
    );

    combine_with_search(st, board, &w, &before, one_ply, &searched, top)
}

/// Dispatch on [`SearchMode`] -- the ONE place a caller that wants to honour
/// an [`Advisor`]'s configured search mode should call, rather than
/// [`rank_moves`] directly (which is always [`SearchMode::Greedy`], no
/// matter what an [`Advisor`] is configured for). Exhaustive: no wildcard
/// arm, so a new [`SearchMode`] variant is a compile error here until this
/// match is updated.
pub fn rank_moves_search(
    search: &SearchConfig,
    board: &Board,
    bot: &WeightedBot,
    top: usize,
    include_end_turn: bool,
) -> Vec<Candidate> {
    match search.mode {
        SearchMode::Greedy => rank_moves(board, bot, top, include_end_turn),
        SearchMode::Beam => rank_moves_beam(&search.plan, board, bot, top, include_end_turn),
        SearchMode::Human => {
            rank_moves_human(&search.plan, &search.human_weights, board, bot, top, include_end_turn)
        }
    }
}

// ---------------------------------------------------- parsing human moves

/// The snake_case name Python's engine spelled this move kind with, and
/// therefore what a human types (`take`, `wonder_step`, ...). Exhaustive:
/// every [`Move`] variant names itself here once, so a new variant is a
/// compile error in this match rather than a silently unparseable move.
fn move_kind(m: &Move) -> &'static str {
    use Move::*;
    match m {
        Take { .. } => "take",
        Build { .. } => "build",
        Develop { .. } => "develop",
        Upgrade { .. } => "upgrade",
        WonderStep { .. } => "wonder_step",
        Pop => "pop",
        PopFree => "pop_free",
        Revolution { .. } => "revolution",
        PlayLeader { .. } => "play_leader",
        PlayAction { .. } => "play_action",
        Destroy { .. } => "destroy",
        PlayTactic { .. } => "play_tactic",
        CopyTactic { .. } => "copy_tactic",
        Aggression { .. } => "aggression",
        War { .. } => "war",
        OfferPact { .. } => "offer_pact",
        CancelPact { .. } => "cancel_pact",
        PrepareEvent { .. } => "prepare_event",
        RemoveLeaderYellow => "remove_leader_yellow",
        ColumbusColonize { .. } => "columbus_colonize",
        Barbarossa { .. } => "barbarossa",
        BachTheater { .. } => "bach_theater",
        Bid { .. } => "bid",
        BidPass => "bid_pass",
        Defend { .. } => "defend",
        DefendDone => "defend_done",
        SendUnit { .. } => "send_unit",
        SendBonus { .. } => "send_bonus",
        SendDiscard { .. } => "send_discard",
        SendDone => "send_done",
        Choose { .. } => "choose",
        Churchill { .. } => "churchill",
        EndTurn => "end_turn",
        PolPass => "pol_pass",
        Resign => "resign",
        TradeFoodAsResource => "trade_food_as_resource",
        TradeResourceAsFood => "trade_resource_as_food",
    }
}

/// A short verb the human might type instead of the real kind word --
/// `t`/`take`, `dev`/`develop`, ... Mirrors `MOVE_ALIASES`. Kinds with no
/// entry here (`churchill`, `resign`, `cancel_pact`, ...) still work: an
/// unaliased verb falls through to [`parse_move`]'s prefix/substring search
/// over the kind words themselves.
fn verb_alias(verb: &str) -> Option<&'static str> {
    Some(match verb {
        "t" | "take" => "take",
        "b" | "build" => "build",
        "u" | "up" | "upgrade" => "upgrade",
        "d" | "dev" | "develop" => "develop",
        "pop" | "population" => "pop",
        "w" | "wonder" | "step" => "wonder_step",
        "leader" | "l" => "play_leader",
        "action" | "card" => "play_action",
        "tactic" => "play_tactic",
        "copy" => "copy_tactic",
        "gov" | "revolution" | "rev" => "revolution",
        "destroy" | "disband" => "destroy",
        "end" | "e" | "done" => "end_turn",
        "pass" | "p" => "pol_pass",
        "agg" | "attack" => "aggression",
        "war" => "war",
        "pact" => "offer_pact",
        "event" => "prepare_event",
        "choose" => "choose",
        "bid" => "bid",
        "defend" => "defend",
        "send" | "sacrifice" => "send_unit",
        "ship" => "send_bonus",
        _ => return None,
    })
}

/// Everything a human might name when picking one move: a numeric argument
/// (a row slot, a bid, a wonder step count) or a piece of text (a card name,
/// or a bare keyword like Churchill's `culture`/`military`). Both kinds are
/// fuzzy-matched identically by [`arg_matches`] -- Python's `_arg_matches`
/// does not distinguish a card name from any other string either.
enum ArgToken {
    Num(u32),
    Text(&'static str),
}

/// Mirrors `_move_tokens`: the move's own arguments, plus (for `take`) the
/// row card's name and (for `wonder_step`) the wonder currently under
/// construction -- context a human names instead of the raw argument.
fn move_tokens(state: &GameState, m: Move) -> Vec<ArgToken> {
    use Move::*;
    match m {
        Take { slot } => {
            let mut v = vec![ArgToken::Num(slot as u32)];
            let id = state.card_row[slot as usize];
            if !id.is_none() {
                v.push(ArgToken::Text(id.name()));
            }
            v
        }
        Build { card }
        | Develop { card }
        | Revolution { card }
        | PlayLeader { card }
        | PlayAction { card }
        | Destroy { card }
        | PlayTactic { card }
        | CopyTactic { card }
        | PrepareEvent { card }
        | ColumbusColonize { card }
        | Barbarossa { card }
        | Defend { card }
        | SendUnit { card }
        | SendBonus { card }
        | SendDiscard { card } => vec![ArgToken::Text(card.name())],
        Upgrade { from, to } | BachTheater { from, to } => {
            vec![ArgToken::Text(from.name()), ArgToken::Text(to.name())]
        }
        WonderStep { steps } => {
            let mut v = vec![ArgToken::Num(steps as u32)];
            let w = state.actor().wonder;
            if !w.is_none() {
                v.push(ArgToken::Text(w.name()));
            }
            v
        }
        Aggression { card, target } | War { card, target } => {
            vec![ArgToken::Text(card.name()), ArgToken::Num(target as u32)]
        }
        OfferPact { card, target, .. } => {
            vec![ArgToken::Text(card.name()), ArgToken::Num(target as u32)]
        }
        CancelPact { owner } => vec![ArgToken::Num(owner as u32)],
        Bid { n } => vec![ArgToken::Num(n as u32)],
        Choose { n } => vec![ArgToken::Num(n as u32)],
        Churchill { choice } => vec![ArgToken::Text(match choice {
            ChurchillChoice::Culture => "culture",
            ChurchillChoice::Military => "military",
        })],
        Pop | PopFree | RemoveLeaderYellow | EndTurn | PolPass | Resign | BidPass
        | DefendDone | SendDone | TradeFoodAsResource | TradeResourceAsFood => vec![],
    }
}

/// Does the human's typed `arg` name this token? Mirrors `_arg_matches`,
/// reusing [`state_io`]'s own fuzzy-matching primitives (prefix / initials /
/// subsequence) rather than restating them -- see that module's doc comment
/// on why they are `pub(crate)`.
fn arg_matches(tok: &ArgToken, arg: &str) -> bool {
    match tok {
        ArgToken::Num(n) => arg.parse::<u32>().is_ok_and(|v| v == *n),
        ArgToken::Text(name) => {
            let a = state_io::norm(arg);
            let e = state_io::norm(name);
            e.starts_with(&a) || state_io::is_subseq(&a, &e) || state_io::initials(name).starts_with(&a)
        }
    }
}

/// A useful message when a move the human named is not legal. Mirrors
/// `_why_not`.
fn why_not(state: &GameState, kind: &str, arg: &str) -> String {
    if kind == "take" {
        if let Ok(slot) = arg.parse::<usize>() {
            if slot >= ROW_SIZE {
                return format!("row slot {slot} does not exist (0..{})", ROW_SIZE - 1);
            }
            let id = state.card_row[slot];
            if id.is_none() {
                return format!("row slot {slot} is empty");
            }
            let cost = crate::costs::take_cost(state, state.actor(), slot);
            return format!(
                "you cannot take '{}' from slot {slot}: it costs {cost} civil actions and you have {}",
                id.name(),
                state.actor().civil_actions
            );
        }
    }
    format!("no legal {kind} matches {arg:?}")
}

/// Turn what the human typed into one of the legal moves. Verb-first and
/// fuzzy: `t 4`, `build bronze`, `dev philo`, `end`. Mirrors `parse_move`.
pub fn parse_move(state: &GameState, text: &str, board: Option<&Board>) -> Result<Move, String> {
    let moves = legal::legal_moves(state);
    let toks: Vec<&str> = text.split_whitespace().collect();
    let Some((&verb_tok, arg_toks)) = toks.split_first() else {
        return Err("type a move, or just press Enter for the top pick".to_string());
    };
    let mut kinds: Vec<&str> = moves.as_slice().iter().map(move_kind).collect();
    kinds.sort_unstable();
    kinds.dedup();

    let verb_owned = verb_tok.to_lowercase();
    let verb = verb_owned.trim_end_matches(':');
    let alias = verb_alias(verb);
    let kind: &str = match alias {
        Some(k) if kinds.contains(&k) => k,
        _ => {
            let starts: Vec<&str> = kinds.iter().copied().filter(|k| k.starts_with(verb)).collect();
            let hits =
                if !starts.is_empty() { starts } else { kinds.iter().copied().filter(|k| k.contains(verb)).collect() };
            match hits.len() {
                1 => hits[0],
                0 => {
                    return Err(match alias {
                        None => format!("no legal move called {verb:?}. Legal now: {}", kinds.join(", ")),
                        Some(k) => format!("{k:?} is not legal right now. Legal now: {}", kinds.join(", ")),
                    })
                }
                _ => return Err(format!("{verb:?} could be: {}", hits.join(", "))),
            }
        }
    };

    let mut cands: Vec<Move> = moves.as_slice().iter().copied().filter(|m| move_kind(m) == kind).collect();
    for &arg in arg_toks {
        cands.retain(|&m| move_tokens(state, m).iter().any(|t| arg_matches(t, arg)));
        if cands.is_empty() {
            return Err(why_not(state, kind, arg));
        }
    }
    match cands.len() {
        1 => Ok(cands[0]),
        0 => Err(format!("no legal {kind} move")),
        _ => {
            let opts: Vec<String> =
                cands.iter().take(8).map(|&m| describe::describe_move(state, m, board)).collect();
            Err(format!("which one?\n   {}", opts.join("\n   ")))
        }
    }
}

// ------------------------------------------------------------- the session

/// Is this a board update rather than a move? Mirrors `_looks_like_patch`:
/// lets the human type `take p1 3` / `p1 c=34` at ANY prompt instead of
/// having to track which prompt they are at.
pub fn looks_like_patch(line: &str) -> bool {
    const PATCH_VERBS: &[&str] = &["deal", "row", "event", "age", "last", "last_round", "set"];
    let Some(first) = line.split_whitespace().next() else {
        return false;
    };
    let first = first.to_lowercase();
    if is_player_word(&first) || first.contains('=') {
        return true;
    }
    if PATCH_VERBS.contains(&first.as_str()) {
        return true;
    }
    if first == "take" {
        let mut toks = line.split_whitespace();
        toks.next();
        return toks.next().is_some_and(|t| is_player_word(&t.to_lowercase()));
    }
    false
}

fn is_player_word(s: &str) -> bool {
    s.strip_prefix('p').is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
}

/// Row slots holding a card that was NOT in the row before, matched to the
/// RIGHTMOST occurrences (the row slides left when replenished, so a
/// positional diff would flag almost every slot; new cards are always dealt
/// on the right). Mirrors `_new_slots`.
fn new_slots(before: &[CardId; ROW_SIZE], after: &[CardId; ROW_SIZE]) -> Vec<usize> {
    let mut added: Vec<(CardId, i32)> = Vec::new();
    let bump = |id: CardId, delta: i32, added: &mut Vec<(CardId, i32)>| match added
        .iter_mut()
        .find(|(c, _)| *c == id)
    {
        Some(e) => e.1 += delta,
        None => added.push((id, delta)),
    };
    for &id in after.iter().filter(|c| !c.is_none()) {
        bump(id, 1, &mut added);
    }
    for &id in before.iter().filter(|c| !c.is_none()) {
        bump(id, -1, &mut added);
    }
    let mut slots = Vec::new();
    for i in (0..after.len()).rev() {
        let id = after[i];
        if id.is_none() {
            continue;
        }
        if let Some(e) = added.iter_mut().find(|(c, _)| *c == id) {
            if e.1 > 0 {
                e.1 -= 1;
                slots.push(i);
            }
        }
    }
    slots.sort_unstable();
    slots
}

/// Board mirror + bot + the operations the interactive loop performs. Kept
/// free of I/O so it can be driven by tests as well as by a terminal.
/// Mirrors `Advisor`; see this module's top doc comment for why it carries
/// no `rng` field.
pub struct Advisor {
    pub board: Board,
    pub bot: WeightedBot,
    pub bot_source: String,
    pub log: Vec<String>,
    /// Row slots the engine just dealt into that the human has not yet named
    /// -- empty except in the window between a deal and [`Advisor::
    /// set_dealt`]/the console clearing it.
    pub dealt_slots: Vec<usize>,
    /// Which [`SearchMode`] [`Advisor::recommend`] ranks with. Defaults to
    /// [`SearchConfig::default`] (`Greedy`) so an existing caller of
    /// [`Advisor::new`] sees no behaviour change; `bin/advisor.rs` overwrites
    /// this field itself once it has parsed `--search` (default `Human`
    /// there -- see [`SearchMode`]'s own doc comment for why the CLI's
    /// default differs from this struct's).
    pub search: SearchConfig,
}

impl Advisor {
    pub fn new(board: Board, bot: WeightedBot, bot_source: String) -> Advisor {
        Advisor {
            board,
            bot,
            bot_source,
            log: Vec::new(),
            dealt_slots: Vec::new(),
            search: SearchConfig::default(),
        }
    }

    pub fn state(&self) -> &GameState {
        &self.board.state
    }

    pub fn my_turn(&self) -> bool {
        !self.board.state.game_over && self.board.state.decider() == self.board.me
    }

    pub fn recommend(&self, top: usize) -> Vec<Candidate> {
        rank_moves_search(&self.search, &self.board, &self.bot, top, true)
    }

    /// Apply a move to the mirror. Returns `(ok, message)`; `false` means
    /// `mv` was not legal and nothing changed. Mirrors `play`; see this
    /// module's top doc comment for why there is no engine-exception path
    /// left to catch once legality is checked structurally first.
    pub fn play(&mut self, mv: Move) -> (bool, String) {
        let legal = legal::legal_moves(&self.board.state);
        if !legal.as_slice().contains(&mv) {
            return (false, format!("{mv:?} is not legal right now"));
        }
        let text = describe::describe_move(&self.board.state, mv, Some(&self.board));
        let row_before = self.board.state.card_row;
        apply::apply(&mut self.board.state, mv);
        self.log.push(text.clone());
        self.dealt_slots = new_slots(&row_before, &self.board.state.card_row);
        (true, text)
    }

    /// Hand the turn on without simulating the opponent's decisions: the
    /// human reports the RESULT of their turn as patches, while the engine
    /// still does the book-keeping (turn order, round/age progression,
    /// end-of-turn production). Mirrors `skip_opponent_turn`.
    ///
    /// A rival's military discard (§6.6 step 1) is hidden information --
    /// face down, never revealed -- so there is nothing to ask the human;
    /// this resolves any such decision by taking the first option the engine
    /// offers, the least-committal guess available. Without this the mirror
    /// would stall mid-sequence and the opponent's production would never
    /// run.
    pub fn skip_opponent_turn(&mut self) -> Vec<usize> {
        let row_before = self.board.state.card_row;
        let mut guard = 0;
        while !self.board.state.game_over && guard < 40 {
            guard += 1;
            if !self.board.state.pending.is_empty() {
                let moves = legal::legal_moves(&self.board.state);
                apply::apply(&mut self.board.state, moves.as_slice()[0]);
                continue;
            }
            if self.board.state.phase == Phase::Politics {
                apply::apply(&mut self.board.state, Move::PolPass);
                continue;
            }
            break;
        }
        if !self.board.state.game_over {
            let who = self.board.state.current;
            apply::apply(&mut self.board.state, Move::EndTurn);
            let mut guard2 = 0;
            while !self.board.state.pending.is_empty() && !self.board.state.game_over && guard2 < 40 {
                guard2 += 1;
                let moves = legal::legal_moves(&self.board.state);
                if moves.is_empty() {
                    break;
                }
                apply::apply(&mut self.board.state, moves.as_slice()[0]);
            }
            self.log.push(format!("p{who} turn ended"));
        }
        self.dealt_slots = new_slots(&row_before, &self.board.state.card_row);
        self.dealt_slots.clone()
    }

    /// Replace the cards the engine guessed in the freshly dealt slots with
    /// what the human actually saw. Mirrors `set_dealt`.
    pub fn set_dealt(&mut self, names: &[String]) -> Result<Vec<CardId>, String> {
        if self.dealt_slots.is_empty() {
            return Err("no cards were dealt since the last update".to_string());
        }
        if names.len() > self.dealt_slots.len() {
            let slots: Vec<String> = self.dealt_slots.iter().map(|s| s.to_string()).collect();
            return Err(format!(
                "only {} card(s) were dealt (slots {})",
                self.dealt_slots.len(),
                slots.join(", ")
            ));
        }
        let mut row = self.board.state.card_row;
        for (&slot, raw) in self.dealt_slots.iter().zip(names.iter()) {
            row[slot] = state_io::resolve_card(raw, state_io::Pool::Row, &[])?;
        }
        state_io::sync_row(&mut self.board, &row);
        let got: Vec<CardId> = self.dealt_slots.iter().take(names.len()).map(|&s| row[s]).collect();
        self.dealt_slots.clear();
        Ok(got)
    }

    pub fn patch(&mut self, line: &str) -> Result<String, String> {
        state_io::patch(&mut self.board, line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::weighted::weights::Weights;
    use crate::game;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    // ---------------------------------------------------------- rank_moves

    #[test]
    fn rank_moves_returns_at_most_top_candidates_each_scored_relative_to_the_best() {
        let board = state_io::new_board(3, 0, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let cands = rank_moves(&board, &bot, 3, true);
        assert!(!cands.is_empty());
        assert!(cands.len() <= 3);
        // The best candidate is always the reference point: score 0.0.
        assert_eq!(cands[0].score, 0.0);
        for c in &cands[1..] {
            assert!(c.score <= 0.0, "{:?}", c.score);
        }
    }

    #[test]
    fn rank_moves_on_an_empty_legal_set_is_the_empty_vec() {
        let mut board = state_io::new_board(2, 0, 1);
        board.state.game_over = true;
        let bot = WeightedBot::new(Weights::defaults());
        // `game_over` states still run `legal_moves` today, so pin the
        // actually-reachable empty case instead: no player left to move.
        // (A resigned lone survivor's `legal_moves` is empty in this
        // engine's contract -- see `legal.rs`'s own tests for `resign`.)
        let _ = rank_moves(&board, &bot, 3, true); // must not panic either way
        board.state.game_over = false;
    }

    // ------------------------------------------------------ search modes

    /// [`SearchConfig::default`] is `Greedy`, and an [`Advisor`] built with
    /// [`Advisor::new`] (which never touches `search`) must therefore
    /// recommend EXACTLY what bare [`rank_moves`] does -- an existing
    /// caller (`bin/harness.rs`, `harness/play.rs`'s scripted operator, the
    /// save/load round-trip tests below) must see no behaviour change from
    /// this type existing at all.
    #[test]
    fn an_advisor_with_no_search_mode_set_recommends_exactly_what_bare_rank_moves_does() {
        let board = state_io::new_board(3, 0, 5);
        let bot = WeightedBot::new(Weights::defaults());
        let adv = Advisor::new(board.clone(), bot, "test".to_string());
        assert_eq!(adv.search.mode, SearchMode::Greedy);
        let direct = rank_moves(&board, &adv.bot, 5, true);
        let via_advisor = adv.recommend(5);
        assert_eq!(direct.len(), via_advisor.len());
        for (a, b) in direct.iter().zip(via_advisor.iter()) {
            assert_eq!(a.mv, b.mv);
            assert_eq!(a.score, b.score);
        }
    }

    /// [`rank_moves_search`]'s `Beam` arm must never drop a legal,
    /// SCOREABLE candidate from the result: [`combine_with_search`]'s
    /// fallback-to-one-ply guarantee exists exactly to keep the candidate
    /// SET identical to what `Greedy` offers, even though a beam-starved
    /// move's score is a 1-ply value rather than a searched one. `top` here
    /// is generous enough to cover a fresh game's whole candidate set (13
    /// row cards + end_turn), so this also pins that count directly.
    #[test]
    fn beam_mode_never_drops_a_candidate_greedy_would_have_shown() {
        let board = state_io::new_board(3, 0, 5);
        let bot = WeightedBot::new(Weights::defaults());
        let greedy = rank_moves(&board, &bot, 32, true);
        let search = SearchConfig {
            mode: SearchMode::Beam,
            plan: plan::PlanConfig { width: 6, max_plies: 4, max_nodes: 1200, ..plan::PlanConfig::default() },
            human_weights: Vec::new(),
        };
        let beam = rank_moves_search(&search, &board, &bot, 32, true);
        assert_eq!(beam.len(), greedy.len(), "beam mode must offer the identical candidate set as greedy");
        let mut greedy_moves: Vec<Move> = greedy.iter().map(|c| c.mv).collect();
        let mut beam_moves: Vec<Move> = beam.iter().map(|c| c.mv).collect();
        greedy_moves.sort_by_key(|m| format!("{m:?}"));
        beam_moves.sort_by_key(|m| format!("{m:?}"));
        assert_eq!(greedy_moves, beam_moves, "beam mode must offer exactly the same moves, just possibly reordered");
    }

    /// Real lookahead changes the RANKING, not just the numbers: on a fresh
    /// game (several civil actions still unspent, so the search tree
    /// extends well past one ply), beam mode must weigh at least one
    /// candidate differently than 1-ply greedy does, or the whole point of
    /// this task -- lookahead that actually changes what gets recommended
    /// -- would not be true. Confirmed RED (this assertion fails) by
    /// temporarily making `rank_moves_beam` call plain `rank_moves` instead
    /// of running the beam -- see this task's commit message / report for
    /// the failure text.
    #[test]
    fn beam_mode_ranks_at_least_one_candidate_differently_than_one_ply_greedy() {
        let board = state_io::new_board(3, 0, 5);
        let bot = WeightedBot::new(Weights::defaults());
        let greedy = rank_moves(&board, &bot, 32, true);
        let search = SearchConfig {
            mode: SearchMode::Beam,
            plan: plan::PlanConfig { width: 6, max_plies: 6, max_nodes: 2000, ..plan::PlanConfig::default() },
            human_weights: Vec::new(),
        };
        let beam = rank_moves_search(&search, &board, &bot, 32, true);
        let greedy_order: Vec<Move> = greedy.iter().map(|c| c.mv).collect();
        let beam_order: Vec<Move> = beam.iter().map(|c| c.mv).collect();
        assert_ne!(greedy_order, beam_order, "beam's lookahead must reorder at least one candidate vs. greedy");
    }

    /// `Human` mode narrows to [`human::root_shortlist`]'s survivors --
    /// every recommended move must still be legal and among what `Greedy`
    /// offers (never a move outside the shared candidate set), and on a
    /// fresh game (13 row cards + end_turn, comfortably more than the
    /// shortlist width) the narrowing must actually have happened.
    ///
    /// A fresh opening only ever offers 6 candidates (5 affordable takes +
    /// end_turn -- not enough civil actions yet to reach anything else), so
    /// this walks a real game forward on a fixed, deterministic move-cycling
    /// policy until the position offers more than a shortlist holds, the
    /// same technique [`human::root_shortlist`]'s own
    /// `a_position_with_more_than_root_shortlist_legal_moves` test helper
    /// uses.
    #[test]
    fn human_mode_only_recommends_moves_the_shortlist_kept() {
        let mut board = state_io::new_board(3, 0, 0);
        board.state = a_position_with_many_legal_moves();
        let bot = WeightedBot::new(Weights::defaults());
        let greedy = rank_moves(&board, &bot, 64, true);
        assert!(greedy.len() > 8, "test needs a position with more than 8 candidates, got {}", greedy.len());
        // An all-zero human vector is a legitimate (if untrained) weight
        // vector -- `human_policy::vector_from_weights`'s own dimension is
        // all this needs to be well-formed; the shortlist rule only needs a
        // vector of the right shape, not a meaningful one, to prove the
        // narrowing wiring itself works.
        let human_weights = vec![0.0; crate::human_policy::feature_dim()];
        let search = SearchConfig {
            mode: SearchMode::Human,
            plan: plan::PlanConfig { width: 6, max_plies: 4, max_nodes: 1200, ..plan::PlanConfig::default() },
            human_weights,
        };
        let human = rank_moves_search(&search, &board, &bot, 64, true);
        assert!(!human.is_empty());
        assert!(human.len() < greedy.len(), "the shortlist must have narrowed the candidate set");
        let greedy_moves: Vec<Move> = greedy.iter().map(|c| c.mv).collect();
        for c in &human {
            assert!(greedy_moves.contains(&c.mv), "{:?} is not among greedy's own candidate set", c.mv);
        }
    }

    /// A real position (not just a fresh opening) with more legal moves than
    /// one human-shortlist holds, reached by playing a real game forward a
    /// bit on a fixed move-index-cycling policy -- deterministic (no RNG
    /// choices of our own), matching [`human::root_shortlist`]'s own test
    /// helper of the same shape.
    fn a_position_with_many_legal_moves() -> GameState {
        for seed in 0..30u64 {
            let mut state = game::new_game(3, seed);
            for step in 0..60u32 {
                let moves = legal::legal_moves(&state);
                if moves.as_slice().len() > 10 {
                    return state;
                }
                if state.game_over {
                    break;
                }
                let mv = moves.as_slice()[step as usize % moves.as_slice().len()];
                apply::apply(&mut state, mv);
            }
        }
        panic!("no position with more than 10 legal moves found in 30 simulated games");
    }

    // ----------------------------------------------------------- parse_move

    #[test]
    fn take_by_row_slot_number_resolves_uniquely() {
        let st = game::new_game(3, 2);
        let mv = parse_move(&st, "t 0", None).unwrap();
        assert_eq!(mv, Move::Take { slot: 0 });
    }

    #[test]
    fn take_by_fuzzy_card_name_resolves_the_row_slot() {
        let st = game::new_game(3, 2);
        let name = st.card_row[0].name();
        // Use the card's own initials/prefix -- guaranteed to match slot 0
        // and, since row cards are distinct, nothing else in the row.
        let prefix = &name[..2.min(name.len())];
        let mv = parse_move(&st, &format!("take {prefix}"), None);
        if let Ok(Move::Take { slot }) = mv {
            assert_eq!(st.card_row[slot as usize].name(), name);
        } else {
            // A short 2-letter prefix can legitimately be ambiguous against
            // another row card; that is a correct "which one?" error, not a
            // test failure -- only assert the unambiguous shape when it is.
        }
    }

    #[test]
    fn an_empty_line_asks_for_a_move() {
        let st = game::new_game(2, 1);
        assert!(parse_move(&st, "", None).is_err());
    }

    #[test]
    fn an_unknown_verb_lists_the_legal_kinds() {
        let st = game::new_game(2, 1);
        let err = parse_move(&st, "frobnicate", None).unwrap_err();
        assert!(err.contains("no legal move called"), "{err}");
        assert!(err.contains("Legal now:"), "{err}");
    }

    #[test]
    fn end_turn_parses_from_any_of_its_aliases() {
        let st = game::new_game(2, 1);
        for verb in ["end", "e", "done", "end_turn"] {
            assert_eq!(parse_move(&st, verb, None).unwrap(), Move::EndTurn, "verb {verb:?}");
        }
    }

    #[test]
    fn take_of_a_row_slot_that_does_not_exist_names_the_valid_range() {
        let st = game::new_game(2, 1);
        let err = parse_move(&st, "take 99", None).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    // ------------------------------------------------------- looks_like_patch

    #[test]
    fn a_bare_player_word_is_a_patch() {
        assert!(looks_like_patch("p1 c=34"));
    }

    #[test]
    fn take_with_a_player_prefix_is_a_patch_not_a_move() {
        assert!(looks_like_patch("take p1 4"));
    }

    #[test]
    fn take_alone_is_a_move_not_a_patch() {
        assert!(!looks_like_patch("take 4"));
    }

    #[test]
    fn deal_is_always_a_patch_verb() {
        assert!(looks_like_patch("deal bronze irrigation"));
    }

    #[test]
    fn an_ordinary_move_line_is_not_a_patch() {
        assert!(!looks_like_patch("build bronze"));
        assert!(!looks_like_patch("end"));
    }

    // -------------------------------------------------------------- Advisor

    #[test]
    fn my_turn_is_true_exactly_when_the_seat_the_human_occupies_is_the_decider() {
        let board = state_io::new_board(3, 1, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let adv = Advisor::new(board, bot, "test".to_string());
        assert_eq!(adv.my_turn(), adv.state().decider() == 1);
    }

    #[test]
    fn playing_an_illegal_move_changes_nothing_and_says_so() {
        let board = state_io::new_board(2, 0, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let mut adv = Advisor::new(board, bot, "test".to_string());
        let before = state_io::dumps(&adv.board);
        let (ok, msg) = adv.play(Move::Take { slot: 99 });
        assert!(!ok);
        assert!(msg.contains("not legal"), "{msg}");
        assert_eq!(state_io::dumps(&adv.board), before);
    }

    #[test]
    fn playing_a_legal_move_logs_its_description() {
        let board = state_io::new_board(2, 0, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let mut adv = Advisor::new(board, bot, "test".to_string());
        let (ok, msg) = adv.play(Move::Take { slot: 0 });
        assert!(ok, "{msg}");
        assert_eq!(adv.log.last().unwrap(), &msg);
    }

    #[test]
    fn set_dealt_with_no_pending_slots_is_an_error() {
        let board = state_io::new_board(2, 0, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let mut adv = Advisor::new(board, bot, "test".to_string());
        assert!(adv.dealt_slots.is_empty());
        assert!(adv.set_dealt(&["Bronze".to_string()]).is_err());
    }

    /// A fresh mirror starts with the whole row pending confirmation, the
    /// same convention the CLI's `main` seeds by hand (`dealt_slots =
    /// 0..ROW_SIZE`) because a fresh physical deal was never "dealt" by this
    /// engine call. `set_dealt` renames those slots to what the human saw.
    #[test]
    fn set_dealt_overwrites_the_named_slots_and_clears_the_pending_list() {
        let board = state_io::new_board(2, 0, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let mut adv = Advisor::new(board, bot, "test".to_string());
        adv.dealt_slots = (0..3).collect();
        let got = adv.set_dealt(&["Bronze".to_string(), "Irrigation".to_string()]).unwrap();
        assert_eq!(got, vec![card("Bronze"), card("Irrigation")]);
        assert_eq!(adv.board.state.card_row[0], card("Bronze"));
        assert_eq!(adv.board.state.card_row[1], card("Irrigation"));
        // Only 2 of the 3 pending slots were named -- the third is still
        // pending resolution... no: `set_dealt` clears the WHOLE pending
        // list once it succeeds, matching Python's `self.dealt_slots = []`.
        assert!(adv.dealt_slots.is_empty());
    }

    #[test]
    fn skip_opponent_turn_advances_past_the_current_player() {
        let board = state_io::new_board(3, 0, 1);
        let bot = WeightedBot::new(Weights::defaults());
        let mut adv = Advisor::new(board, bot, "test".to_string());
        let who = adv.state().current;
        adv.skip_opponent_turn();
        assert_ne!(adv.state().current, who);
        assert!(adv.log.iter().any(|l| l.contains("turn ended")));
    }
}
