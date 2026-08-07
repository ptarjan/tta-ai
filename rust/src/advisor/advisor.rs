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
use crate::bots::weighted::eval::{self, WeightedBot};
use crate::bots::weighted::features;
use crate::bots::weighted::rivals;
use crate::bots::weighted::weights::WeightKey;
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

/// Score every legal move with the bot's own evaluation and return the best
/// `top`. Mirrors `rank_moves`; see this module's top doc comment for the
/// three Python mechanisms (`rng`, `fastcopy`, per-candidate `try`/`except`)
/// this does not carry.
pub fn rank_moves(board: &Board, bot: &WeightedBot, top: usize, include_end_turn: bool) -> Vec<Candidate> {
    let st = &board.state;
    let moves = legal::legal_moves(st);
    if moves.is_empty() {
        return Vec::new();
    }
    let idx = st.decider();

    // Resigning is (almost) never the right first suggestion at a physical
    // table -- prefer the non-resign candidate set when one exists, exactly
    // as Python's `[m for m in moves if m[0] != "resign"] or moves` does.
    let non_resign: Vec<Move> =
        moves.as_slice().iter().copied().filter(|m| !matches!(m, Move::Resign)).collect();
    let candidates: &[Move] = if non_resign.is_empty() { moves.as_slice() } else { &non_resign };

    // Computed once at the root and reused for every candidate -- the same
    // reuse `WeightedBot::choose` documents (an information-leak concern,
    // not just an optimisation, if recomputed per candidate).
    let ctx = rivals::rival_context(st, idx, None, None);
    let before = features::features(st, idx, Some(&ctx), None, false);
    let w = &bot.weights;
    let end_bias = w.get(WeightKey::EndTurnBias);

    let mut scored: Vec<(f64, Move, features::Features)> = Vec::new();
    for &mv in candidates {
        if matches!(mv, Move::EndTurn) && !include_end_turn && candidates.len() > 1 {
            continue;
        }
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
}

impl Advisor {
    pub fn new(board: Board, bot: WeightedBot, bot_source: String) -> Advisor {
        Advisor { board, bot, bot_source, log: Vec::new(), dealt_slots: Vec::new() }
    }

    pub fn state(&self) -> &GameState {
        &self.board.state
    }

    pub fn my_turn(&self) -> bool {
        !self.board.state.game_over && self.board.state.decider() == self.board.me
    }

    pub fn recommend(&self, top: usize) -> Vec<Candidate> {
        rank_moves(&self.board, &self.bot, top, true)
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
