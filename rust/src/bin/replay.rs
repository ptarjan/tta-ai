//! `replay` -- the game-state reconstruction spike (`docs/REPLAY.md`).
//!
//! For a given BGO human game id, walks `sources/bgo/journals/<id>.tsv` in
//! order, translates each line into the corresponding engine [`Move`], and
//! applies it through the REAL engine (`legal::legal_moves`, `apply::apply`)
//! -- never by hand-mutating `GameState` to force a match. At every step the
//! human's action must appear in `legal_moves()` for the reconstructed
//! state; when it does not, that is recorded as a structured [`Mismatch`]
//! and the game stops there.
//!
//! ```text
//! tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals
//! cargo run --profile difftest --bin replay -- \
//!     sources/bgo/index.tsv /tmp/bgo-journals/journals <game_id> [game_id ...]
//! ```
//!
//! # What is RECONSTRUCTED vs SIMULATED
//!
//! The journal cannot tell this binary everything the engine needs -- in
//! particular the true civil/military deck shuffle order, and what sat in a
//! player's hand before it was played. This file draws a hard line between
//! two kinds of state, and the fidelity report (`docs/REPLAY.md`) reports
//! against that line explicitly:
//!
//! - **RECONSTRUCTED**: every card identity a human is ever observed to
//!   take, build, develop, play, elect, declare, propose, colonize, bid on,
//!   or destroy. These come straight from the journal text (reusing
//!   `tta::corpus::classify`, the same classifier `corpuscensus.rs` already
//!   validated at 99.99% line coverage) and are applied as real engine
//!   `Move`s, checked against `legal_moves` at every step.
//! - **SIMULATED**: everything the engine needs to hold a legal, complete
//!   `GameState` that the journal never reveals -- unrevealed card-row
//!   slots, the civil/military deck order beyond what's been drawn, and a
//!   player's hand contents before they are observed playing them. This
//!   binary seeds these from `game::new_game`'s ordinary (fictional) shuffle
//!   and OVERWRITES a slot/hand entry with the real observed card the
//!   instant that slot/card is ever taken or played -- "grounding" it, in
//!   this file's terms. An ungrounded slot's identity is never validated
//!   against anything and never claimed to be historically accurate; only
//!   its EXISTENCE (there was some card there, costing some action) is real.
//!
//! # Event/Territory preparation: the one inference this file makes
//!
//! `Move::PrepareEvent` (the political action that queues a drawn Event or
//! Territory card to fire later) has NO journal line of its own --
//! `corpus.rs`'s module doc already establishes this for `corpuscensus.rs`'s
//! purposes; it matters much more here, because replay cannot progress past
//! a Politics-phase decision without resolving it somehow. Every
//! `PrepareEvent` call causes exactly one `events::reveal_current_event`
//! (`rust/src/events.rs`), so this file pre-scans each journal once,
//! collecting the exact card named in every `"...Current event:; <Age> /
//! <Name>; ..."` line (`event_reveals`, FIFO). Whenever a player's Politics
//! decision cannot be explained by an explicit textual action (pass,
//! revolution, war, aggression, pact offer), this file infers a hidden
//! `PrepareEvent`, grants that player a placeholder Event-kind card (its
//! identity is never checked against anything -- see SIMULATED above), and
//! forces `state.current_events` to reveal exactly the next journal-observed
//! event/territory so the resolution the journal shows next lines up.
//!
//! This reproduces the right cards firing in the right ORDER, but NOT on the
//! historically correct turn and NOT by the historically correct preparer:
//! both are permanently unrecoverable from BGO's journal format (it never
//! logs preparation, only firing). `docs/REPLAY.md` states this plainly as
//! a boundary on what a later agreement analysis may claim about political
//! decisions specifically.
//!
//! # What this file gives up on, and why
//!
//! - **Discard** (§6.6 hand-limit, and any other forced military discard):
//!   BGO's journal logs only a count (`"<Color> discards N cards"`), never
//!   which cards -- `corpus.rs`'s own doc establishes this. Genuinely
//!   unrecoverable; this file stops the game there rather than guess.
//! - **Aggression defense** with any committed defense cards: BGO logs only
//!   a count (`"<Color> defends N Defense card(s) played"`), never which
//!   ones. Zero committed cards is unambiguous (`DefendDone` immediately);
//!   any positive count is unrecoverable, and stops the game.
//! - **`PutBack`** (a human undoing their own `Take` via BGO's client-side
//!   undo): there is no `Move` for this in the engine at all -- `moves.rs`'s
//!   variant list has no "untake". Stops the game rather than hand-mutate
//!   state to fake a reversal that was never a real game action.
//! - **Colonization sacrifice specifics**: `"Sacrificed Units:; ..."` DOES
//!   name exact identities, but resolving `Pending::Colonize`'s branching
//!   `SendUnit`/`SendBonus`/`SendDiscard` choices against that list is not
//!   implemented in this pass -- this file auto-drains colonization by
//!   picking the engine's own first offered option at each step
//!   (`colonize_moves`'s own ordering) until the force clears. This keeps
//!   the game running and gets the CULTURE/RESOURCE totals from the
//!   colonize card right (those come from the reveal, not the sacrifice),
//!   but does NOT verify which units were spent -- flagged as an
//!   approximation, not a validated step, in every game where it fires.

use std::collections::{HashMap, VecDeque};
use std::env;
use std::fs;
use std::process::ExitCode;

use tta::corpus::{
    self, actor_and_rest, build_card_index, classify, longest_known_card_prefix, ActionClass,
    Classified, Color, GameMeta, LineOutcome,
};
use tta::moves::PactSide;
use tta::state::{ChoiceKind, ChoiceOption, GameState, Keyword, Pending, Phase};
use tta::{apply, costs, game, legal, CardId, CardType, Move};

// ---------------------------------------------------------------------
// Journal line
// ---------------------------------------------------------------------

/// One journal row, still borrowing from the file's text.
struct Line<'a> {
    lineno: usize,
    age: &'a str,
    round: &'a str,
    text: &'a str,
}

fn parse_lines(journal_text: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    for (i, line) in journal_text.lines().enumerate() {
        if i == 0 || line.is_empty() {
            continue; // header / blank
        }
        let fields: Vec<&str> = line.splitn(5, '\t').collect();
        if fields.len() != 5 {
            continue; // malformed row, same tolerance corpuscensus uses
        }
        out.push(Line { lineno: i + 1, age: fields[2], round: fields[3], text: fields[4] });
    }
    out
}

// ---------------------------------------------------------------------
// Mismatch categories
// ---------------------------------------------------------------------

/// Why a game's replay stopped before the journal ran out. Every variant
/// corresponds to one row of `docs/REPLAY.md`'s mismatch table.
// Every field here is read through `{:?}` in `print_result`, not by field
// access -- `dead_code` can't see through `Debug`, so it's silenced rather
// than worked around with unit-typed fields that would lose the detail.
#[derive(Debug)]
#[allow(dead_code)]
enum MismatchKind {
    /// A hidden piece of state (discard identity, defense-card identity,
    /// BGO's client-side undo) genuinely cannot be recovered from the
    /// journal; see this file's module doc.
    UnrecoverableHiddenInfo(String),
    /// The `Move` this binary constructed from the line is not present in
    /// `legal_moves()` for the reconstructed state -- either a parser gap
    /// (this binary mis-translated the line) or a genuine engine rules
    /// mismatch (flagged separately once triaged).
    IllegalMove { attempted: String, legal_moves: String },
    /// `resolve_intervening` could not make progress -- the decider the
    /// state demands next never matches any upcoming actor, or a pending
    /// decision kind it does not know how to auto-resolve blocks it.
    StuckPending(String),
    /// This binary could not parse a field the journal text was expected to
    /// carry (a target colour, an action-point cost, a bid amount, ...).
    ParserGap(String),
}

struct Mismatch {
    lineno: usize,
    age: String,
    round: String,
    raw_text: String,
    kind: MismatchKind,
}

// ---------------------------------------------------------------------
// Replay state
// ---------------------------------------------------------------------

struct Replayer<'a> {
    card_index: &'a HashMap<&'static str, CardId>,
    state: GameState,
    /// Which of the 13 card-row slots currently hold a card whose identity
    /// is REAL (observed) rather than SIMULATED filler from `new_game`'s
    /// fictional deal. See the module doc's "RECONSTRUCTED vs SIMULATED".
    row_grounded: [bool; 13],
    /// FIFO of the exact card revealed by each `"...Current event:; ..."`
    /// line, pre-scanned once per game. See the module doc's "Event/
    /// Territory preparation" section.
    event_reveals: VecDeque<CardId>,
    /// Whether any colonization in this game was resolved by the
    /// approximate auto-drain rather than a verified sacrifice match.
    colonize_approximated: bool,
    /// Number of actionable (non-bookkeeping) journal lines consumed.
    actions_consumed: usize,
    /// Whether seat `i` has ever been credited a `"draws N military
    /// card(s)"` clause yet (parsed off their own `EndTurn` lines as they
    /// are applied). BGO deals no military cards at all until a player's
    /// FIRST end-of-turn draw; before that, their military hand is
    /// genuinely empty, so a Politics-phase decision with no explicit
    /// textual action is a forced, trivially-logged-or-not pass -- NOT a
    /// hidden `PrepareEvent` (which needs an Event/Territory card in hand
    /// that cannot possibly exist yet). Without this check the inference in
    /// `resolve_hidden_politics_decision` mistakes round 2's forced pass for
    /// a real preparation, consuming the wrong entry off `event_reveals`
    /// and misattributing every event downstream -- found by testing
    /// against a real 2p game (`docs/REPLAY.md`).
    has_drawn_military: [bool; 4],
}

/// A placeholder Event-kind card used to satisfy `Move::PrepareEvent`'s
/// hand requirement when this binary infers a hidden preparation. Its
/// identity is never checked against anything (see module doc): the engine
/// only needs SOME Event/Territory-kind card in the decider's military hand
/// to make `PrepareEvent` legal at all, and this file immediately overrides
/// `current_events` with the journal-observed card that must actually be
/// revealed. Picked once, lazily, from `tta::CARDS`.
fn filler_event_card() -> CardId {
    (0..tta::CARDS.len() as u16)
        .map(CardId)
        .find(|id| id.kind() == CardType::Event)
        .expect("the base game table has at least one Event card")
}

impl<'a> Replayer<'a> {
    fn new(card_index: &'a HashMap<&'static str, CardId>, num_players: u8, event_reveals: VecDeque<CardId>) -> Self {
        // The seed is thrown away semantically -- every field it determines
        // (deck order, starting row/hand contents) is SIMULATED filler this
        // binary overwrites the instant a slot/hand entry is observed. It is
        // fixed (not random) purely so a run is reproducible byte-for-byte.
        let state = game::new_game(num_players, 0xC0FFEE);
        Replayer {
            card_index,
            state,
            row_grounded: [false; 13],
            event_reveals,
            colonize_approximated: false,
            actions_consumed: 0,
            has_drawn_military: [false; 4],
        }
    }

    /// Make `legal_moves(&self.state)` actually offer `mv`, resolving every
    /// decision that stands between the current state and `expected_actor`
    /// getting to move -- auto-declining stale pact offers, auto-passing
    /// auctions nobody bid on, auto-draining a colonization force, and (the
    /// one real inference this file makes) resolving a Politics-phase
    /// decision that has no explicit textual action as a hidden
    /// `PrepareEvent`. See the module doc for the justification of each.
    /// `next_line_explains_own_politics` tells this whether the journal line
    /// about to be translated is itself `expected_actor`'s explicit
    /// political action (pass/revolution/war/aggression/pact) -- if so, and
    /// it is exactly `expected_actor`'s own turn to make that decision
    /// (`phase == Politics`, no pending, `decider == expected_actor`), this
    /// returns immediately and lets the caller apply that line normally.
    /// Otherwise a Politics-phase stop at `expected_actor` with no
    /// explaining line means THEY had a hidden `PrepareEvent` -- inferred
    /// the same way as for any other player (see the module doc).
    fn resolve_intervening(
        &mut self,
        expected_actor: u8,
        upcoming: (ActionClass, Option<CardId>),
        next_line_explains_own_politics: bool,
    ) -> Result<(), MismatchKind> {
        for _ in 0..200 {
            let decider = self.state.decider();
            // A `Pending::Choice(FreeBuild)` (an event's "each player with
            // an unused worker may immediately build X for free" -- e.g.
            // "Development of Religion") is left open regardless of WHOSE
            // turn it nominally is, is not gated on `phase`, and a human
            // DECLINING it (`Skip`) leaves no journal trace at all -- the
            // same silent-response shape as a Politics-phase pass, just for
            // a different pending kind. Drained here, ahead of the
            // decider-equality check below, exactly like the Politics case:
            // if the upcoming line is a build that matches one of its
            // options, stop here and let `apply_one`'s Build handling
            // resolve it (it needs the parsed card, which this function
            // doesn't have reason to duplicate); otherwise assume `Skip` and
            // keep draining (there can be one such pending per qualifying
            // player, queued back to back) -- found by testing against a
            // real 3p game (`docs/REPLAY.md`).
            if let Some(Pending::Choice(c)) = self.state.pending.top().cloned() {
                if matches!(c.kind, ChoiceKind::FreeBuild) {
                    let matches_upcoming = decider == expected_actor
                        && matches!(upcoming.0, ActionClass::BuildBuilding | ActionClass::BuildUnit)
                        && upcoming.1.is_some_and(|want| {
                            c.options.as_slice().iter().any(|o| matches!(o, ChoiceOption::Card(id) if *id == want))
                        });
                    if matches_upcoming {
                        return Ok(());
                    }
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Word(Keyword::Skip)))
                        .ok_or_else(|| MismatchKind::StuckPending("FreeBuild choice has no Skip option".into()))?;
                    apply::apply(&mut self.state, Move::Choose { n: n as u8 });
                    continue;
                }
            }
            if decider == expected_actor {
                let own_politics_decision = self.state.phase == Phase::Politics && self.state.pending.is_empty();
                if !own_politics_decision || next_line_explains_own_politics {
                    return Ok(());
                }
                self.resolve_hidden_politics_decision(decider)?;
                continue;
            }
            match self.state.pending.top().cloned() {
                Some(Pending::Choice(c)) if matches!(c.kind, ChoiceKind::PactOffer { .. }) => {
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Word(Keyword::Refuse)))
                        .ok_or_else(|| MismatchKind::StuckPending("PactOffer choice has no Refuse option".into()))?;
                    apply::apply(&mut self.state, Move::Choose { n: n as u8 });
                }
                Some(Pending::Auction(_)) => {
                    apply::apply(&mut self.state, Move::BidPass);
                }
                Some(Pending::Colonize(_)) => {
                    self.auto_drain_colonize()?;
                }
                Some(Pending::Defense(_)) => {
                    // A live defense should always be resolved inline right
                    // after the triggering Aggression (see
                    // `apply_play_aggression`); reaching one here means an
                    // earlier aggression's defense was left open, which this
                    // binary treats as a bug in its own bookkeeping, not a
                    // journal fact -- fail loudly rather than silently
                    // picking a defense.
                    return Err(MismatchKind::StuckPending(
                        "a Defense pending was left open across an action boundary".into(),
                    ));
                }
                Some(Pending::Choice(c)) => {
                    return Err(MismatchKind::StuckPending(format!(
                        "no auto-resolution for pending choice {:?} (decider {decider}, options {})",
                        c.kind,
                        c.options.len()
                    )));
                }
                None => {
                    if self.state.phase == Phase::Politics && decider != expected_actor {
                        self.resolve_hidden_politics_decision(decider)?;
                    } else {
                        return Err(MismatchKind::StuckPending(format!(
                            "decider {decider} != expected actor {expected_actor}, phase {:?}, no pending",
                            self.state.phase
                        )));
                    }
                }
            }
        }
        Err(MismatchKind::StuckPending("resolve_intervening did not converge in 200 steps".into()))
    }

    /// Resolves a Politics-phase decision for `decider` that has no
    /// explicit textual action. Two real causes look identical in the
    /// journal (nothing is logged either way): a genuinely forced pass
    /// (no military-hand card exists to do anything else with -- true
    /// before `decider`'s first `"draws N military card(s)"` credit, see
    /// `has_drawn_military`'s doc) and a hidden `PrepareEvent`. This
    /// disambiguates using that one text-derivable fact rather than
    /// guessing; see the module doc's "Event/Territory preparation"
    /// section for the inference itself once it applies.
    fn resolve_hidden_politics_decision(&mut self, decider: u8) -> Result<(), MismatchKind> {
        if !self.has_drawn_military[decider as usize] {
            return self.try_apply(Move::PolPass);
        }
        let filler = filler_event_card();
        self.state.players[decider as usize].hand_military.push(filler);
        if let Some(want) = self.event_reveals.pop_front() {
            // `current_events.pop()` takes from the END (see `CardList`), so
            // pushing `want` last guarantees it is exactly what
            // `events::reveal_current_event` reveals for this PrepareEvent.
            self.state.current_events.push(want);
        }
        let mv = Move::PrepareEvent { card: filler };
        let legal = legal::legal_moves(&self.state);
        if !legal.as_slice().contains(&mv) {
            return Err(MismatchKind::IllegalMove {
                attempted: format!("{mv:?} (inferred hidden preparation for player {decider})"),
                legal_moves: format!("{:?}", legal.as_slice()),
            });
        }
        apply::apply(&mut self.state, mv);
        Ok(())
    }

    /// Repeatedly pick the engine's own first-offered continuation of an
    /// open `Pending::Colonize` until it clears. Not verified against
    /// `"Sacrificed Units:; ..."` -- see the module doc's "gives up on"
    /// section. Records that this game's colonize sacrifice is approximate.
    fn auto_drain_colonize(&mut self) -> Result<(), MismatchKind> {
        self.colonize_approximated = true;
        for _ in 0..64 {
            if !matches!(self.state.pending.top(), Some(Pending::Colonize(_))) {
                return Ok(());
            }
            let legal = legal::legal_moves(&self.state);
            let Some(&mv) = legal.as_slice().first() else {
                return Err(MismatchKind::StuckPending("Pending::Colonize offered zero moves".into()));
            };
            apply::apply(&mut self.state, mv);
        }
        Err(MismatchKind::StuckPending("colonize force did not resolve in 64 steps".into()))
    }

    /// Ensure `card` is in `actor`'s military hand, granting it directly if
    /// the journal is the first place this binary has ever seen it (the
    /// task's suggested approach for hidden military-hand info: "grant a
    /// player the card they are observed to play"). A no-op if it is
    /// already there (e.g. the fictional per-round deal happened to match).
    fn ground_military_hand(&mut self, actor: u8, card: CardId) {
        let hand = &mut self.state.players[actor as usize].hand_military;
        if !hand.contains(card) {
            hand.push(card);
        }
    }

    /// Find (or force) the row slot `card` should be taken from. Prefers a
    /// slot already grounded to `card` (a filler had NOT been placed over
    /// it because this exact card was placed there by an earlier call);
    /// otherwise forces `card` into whichever ungrounded slot's `take_cost`
    /// matches the journal's stated action-point cost, or the first
    /// ungrounded slot if no cost was parseable. See the module doc.
    fn ground_row_slot(&mut self, actor: u8, card: CardId, observed_cost: Option<i32>) -> Option<u8> {
        // Only trust a slot already showing `card` if THIS binary put it
        // there (`row_grounded`). `new_game`'s fictional deal can coincide
        // with the real card by pure chance (13 cards drawn from a 236-card
        // table isn't rare to collide on); an ungrounded coincidence has no
        // bearing on which slot the human actually paid for, so it must
        // still be forced by cost like any other take.
        if let Some(idx) = (0..13).find(|&i| self.row_grounded[i] && self.state.card_row[i] == card) {
            return Some(idx as u8);
        }
        let ungrounded: Vec<usize> = (0..13)
            .filter(|&i| !self.row_grounded[i] && !self.state.card_row[i].is_none())
            .collect();
        if let Some(want) = observed_cost {
            for &i in &ungrounded {
                let saved = self.state.card_row[i];
                self.state.card_row[i] = card;
                let cost = costs::take_cost(&self.state, &self.state.players[actor as usize], i);
                if cost == want {
                    self.row_grounded[i] = true;
                    return Some(i as u8);
                }
                self.state.card_row[i] = saved;
            }
        }
        let i = *ungrounded.first()?;
        self.state.card_row[i] = card;
        self.row_grounded[i] = true;
        Some(i as u8)
    }

    /// Apply `mv` if legal; otherwise build an `IllegalMove` mismatch.
    fn try_apply(&mut self, mv: Move) -> Result<(), MismatchKind> {
        let legal = legal::legal_moves(&self.state);
        if !legal.as_slice().contains(&mv) {
            return Err(MismatchKind::IllegalMove {
                attempted: format!("{mv:?}"),
                legal_moves: format!("{:?}", legal.as_slice()),
            });
        }
        apply::apply(&mut self.state, mv);
        Ok(())
    }
}

// ---------------------------------------------------------------------
// Small text helpers replay.rs needs beyond tta::corpus::classify
// ---------------------------------------------------------------------

/// Sums every `"uses N civil action"` / `"uses N military action"` clause in
/// a line -- the total action-point cost BGO printed for a take/declare/
/// aggression/tactic line, used to disambiguate which row slot (or to
/// sanity-check a cost) since the journal never prints a slot index
/// directly.
/// The number right after the FIRST `"spends N resource"` in `text`, if any
/// -- used to cross-check a build's stated cost against this binary's own
/// `costs::build_cost_for`. BGO logs `"builds X using Y"` for at least two
/// real discount sources (a per-age blue-tech `buildDiscount` pool, and
/// William Shakespeare's library/theater discount are both modeled in
/// `costs::build_cost_for` already) but ALSO for at least one this binary
/// does not model (observed against a real 2p game, `docs/REPLAY.md`: a
/// build tagged `"using Urban Growth"` costing 1 less than the printed/
/// computed price with no leader or blue-tech explaining it). Silently
/// applying the wrong (higher) cost drains the reconstructed economy by the
/// missed discount every time it fires, which does not fail AT that build --
/// it fails several actions later when the shortfall finally blocks
/// something, far from its real cause. Catching the mismatch at the source
/// turns a confusing cascading failure into one clearly labelled stop.
fn spent_resources(text: &str) -> Option<i32> {
    let p = text.find(" spends ")?;
    let rest = &text[p + " spends ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    if !rest[digits_end..].starts_with(" resource") {
        return None; // "spends N food" etc. -- not this check's business
    }
    rest[..digits_end].parse().ok()
}

fn total_action_cost(text: &str) -> Option<i32> {
    let mut total = 0i32;
    let mut found = false;
    let mut rest = text;
    while let Some(p) = rest.find("uses ") {
        rest = &rest[p + "uses ".len()..];
        let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
        if digits_end == 0 {
            continue;
        }
        let n: i32 = rest[..digits_end].parse().ok()?;
        let after = &rest[digits_end..];
        if after.starts_with(" civil action") || after.starts_with(" military action") {
            total += n;
            found = true;
        }
        rest = after;
    }
    found.then_some(total)
}

/// Finds a known colour immediately following `marker` in `text` (e.g.
/// `marker = " on "` for a war declaration's target, `" against "` for an
/// aggression's, `" to "` for a pact proposal's).
fn color_after(text: &str, marker: &str) -> Option<Color> {
    let pos = text.find(marker)?;
    let rest = &text[pos + marker.len()..];
    Color::parse(rest.split(|c: char| !c.is_ascii_alphabetic()).next()?)
}

/// `"<N> stage(s) of <Wonder>"` -- the stage count, which
/// `tta::corpus::classify` does not surface (it only returns the wonder
/// card).
fn wonder_stage_count(rest_after_builds: &str) -> Option<u8> {
    let digits_end = rest_after_builds.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    rest_after_builds[..digits_end].parse().ok()
}

/// `"<From> to <To> ..."` -- the FROM card, which `classify` also does not
/// surface (it only returns the TO card, since that decides
/// UpgradeUnit/UpgradeProduction).
fn upgrade_from_card(card_index: &HashMap<&'static str, CardId>, rest_after_upgrades: &str) -> Option<CardId> {
    longest_known_card_prefix(card_index, rest_after_upgrades).map(|(id, _)| id)
}

/// `"<Actor> is A"` / `"<Actor> is B"` -- which side a pact proposer took.
/// Both are legal moves when the card has distinct sides; `Unspecified`
/// covers cards that don't.
fn pact_side(text: &str, actor: Color, card_id: CardId) -> PactSide {
    let has_sides = card_id.get().special.iter().any(|s| matches!(s, tta::cards::Special::A(_)))
        && card_id.get().special.iter().any(|s| matches!(s, tta::cards::Special::B(_)));
    if !has_sides {
        return PactSide::Unspecified;
    }
    let marker = format!("{} is A", actor.as_str());
    if text.contains(&marker) {
        PactSide::A
    } else {
        PactSide::B
    }
}

/// `"<Color> defends N Defense card..."` -- the count of committed defense
/// cards on the line right after an Aggression. `0` is unambiguous
/// (`DefendDone`); any positive count is unrecoverable (identities never
/// printed) -- see the module doc.
fn defends_count(text: &str) -> Option<i32> {
    let (_, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("defends ")?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    rest[..digits_end].parse().ok()
}

/// The event/territory name out of `"...Current event:; <Age> / <Name>; ..."`.
fn current_event_name(text: &str) -> Option<&str> {
    let p = text.find("Current event:; ")?;
    let rest = &text[p + "Current event:; ".len()..];
    let slash = rest.find(" / ")?;
    let after_slash = &rest[slash + 3..];
    let end = after_slash.find(';').unwrap_or(after_slash.len());
    Some(after_slash[..end].trim())
}

// ---------------------------------------------------------------------
// Pre-scan: the event/territory reveal FIFO
// ---------------------------------------------------------------------

fn prescan_event_reveals(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> VecDeque<CardId> {
    let mut out = VecDeque::new();
    for line in lines {
        if let Some((_, rest)) = actor_and_rest(line.text) {
            if rest.starts_with("plays event") {
                if let Some(name) = current_event_name(line.text) {
                    if let Some(&id) = card_index.get(name) {
                        out.push_back(id);
                    }
                }
            }
        }
    }
    out
}

/// Line indices to skip entirely because they are a `TakeCard` immediately
/// (allowing only intervening bookkeeping lines) undone by a same-actor,
/// same-card `PutBack` -- BGO's client-side undo (`corpus.rs`'s module doc:
/// "~8% of raw takes are a human undoing their own take within the same
/// turn"). Rather than modelling `PutBack` as an engine `Move` (there is
/// none -- see this file's module doc), the take that never should have
/// counted is simply never applied: both journal lines are skipped as a
/// pair, which is the exact meaning of "take it back."
fn prescan_putback_skips(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> std::collections::HashSet<usize> {
    let mut skip = std::collections::HashSet::new();
    let mut last_take: Option<(usize, Color, CardId)> = None;
    for (i, line) in lines.iter().enumerate() {
        let LineOutcome::Action(Classified { class, card }) = classify(card_index, line.text) else {
            continue; // bookkeeping between a take and its put-back is fine
        };
        let Some((actor, _)) = actor_and_rest(line.text) else { continue };
        match (class, card) {
            (ActionClass::TakeCard, Some(c)) => last_take = Some((i, actor, c)),
            (ActionClass::PutBack, Some(c)) => {
                if let Some((take_i, take_actor, take_card)) = last_take {
                    if take_actor == actor && take_card == c {
                        skip.insert(take_i);
                        skip.insert(i);
                    }
                }
                last_take = None;
            }
            _ => last_take = None,
        }
    }
    skip
}

// ---------------------------------------------------------------------
// Per-game replay
// ---------------------------------------------------------------------

struct GameResult {
    id: String,
    players: u8,
    actions_consumed: usize,
    completed: bool,
    mismatch: Option<Mismatch>,
    colonize_approximated: bool,
    engine_scores: Option<Vec<i32>>,
    index_scores: Vec<i32>,
}

fn replay_game(meta: &GameMeta, journal_text: &str, card_index: &HashMap<&'static str, CardId>) -> GameResult {
    let lines = parse_lines(journal_text);
    let event_reveals = prescan_event_reveals(&lines, card_index);
    let putback_skips = prescan_putback_skips(&lines, card_index);
    let mut r = Replayer::new(card_index, meta.players, event_reveals);

    let mut mismatch: Option<Mismatch> = None;
    let mut completed = false;

    'lines: for (i, line) in lines.iter().enumerate() {
        if line.text.starts_with("End of game") {
            completed = true;
            break;
        }
        if putback_skips.contains(&i) {
            continue;
        }
        let outcome = classify(card_index, line.text);
        let LineOutcome::Action(Classified { class, card }) = outcome else {
            continue; // bookkeeping / unclassified: no move to apply
        };
        let Some((actor_color, rest)) = actor_and_rest(line.text) else {
            // EndTurn lines start with "End turn", no leading colour --
            // the actor is whoever the engine currently has as `current`.
            if class == ActionClass::EndTurn {
                let actor = r.state.current;
                if let Err(kind) = r
                    .resolve_intervening(actor, (class, None), false)
                    .and_then(|()| r.try_apply(Move::EndTurn))
                {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
                }
                if line.text.contains("draws ") && line.text.contains(" military card") {
                    r.has_drawn_military[actor as usize] = true;
                }
                r.actions_consumed += 1;
                continue;
            }
            mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("action line has no leading colour and is not EndTurn".into())));
            break 'lines;
        };
        let actor = actor_color.seat();
        if actor >= meta.players {
            mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("actor colour {actor_color:?} outside {}p seating", meta.players))));
            break 'lines;
        }

        let explains_own_politics = matches!(
            class,
            ActionClass::Pass
                | ActionClass::ChangeGovernment
                | ActionClass::DeclareWar
                | ActionClass::PlayAggression
                | ActionClass::ProposePact
        );
        if let Err(kind) = r.resolve_intervening(actor, (class, card), explains_own_politics) {
            mismatch = Some(mk_mismatch(line, kind));
            break 'lines;
        }

        let next_text = lines.get(i + 1).map(|l| l.text);
        let result = apply_one(&mut r, actor, class, card, rest, line.text, next_text);
        match result {
            Ok(()) => {
                r.actions_consumed += 1;
            }
            Err(kind) => {
                mismatch = Some(mk_mismatch(line, kind));
                break 'lines;
            }
        }
    }

    if mismatch.is_none() && !completed {
        // Journal ran out without an explicit "End of game" line (some
        // journals end mid-stream, e.g. a resignation) -- not a mismatch,
        // just not a verified-complete replay either.
    }

    let engine_scores = if completed && r.state.game_over {
        Some(game::scores(&r.state))
    } else {
        None
    };

    GameResult {
        id: meta.id.clone(),
        players: meta.players,
        actions_consumed: r.actions_consumed,
        completed: completed && mismatch.is_none(),
        mismatch,
        colonize_approximated: r.colonize_approximated,
        engine_scores,
        index_scores: meta.scores.clone(),
    }
}

fn mk_mismatch(line: &Line, kind: MismatchKind) -> Mismatch {
    Mismatch {
        lineno: line.lineno,
        age: line.age.to_string(),
        round: line.round.to_string(),
        raw_text: line.text.to_string(),
        kind,
    }
}

/// If `rest` (the `"builds ..."` line, text after the actor's colour) names
/// a `"using <Card>"` discount source that is a `FreeCivilAction`-granting
/// Action card currently in `actor`'s hand, plays it (`Move::PlayAction`)
/// and returns the `Move::Choose` that resolves the resulting
/// `Pending::Choice(FreeCivil)` onto building `built_card` -- the caller
/// applies that returned move. Returns `Ok(None)` when there is no such
/// discount source named (falls back to a plain `Move::Build`) rather than
/// when there IS one but something about it fails to resolve (that's an
/// `Err`, not a silent fallback -- see the module doc's "gives up on" list
/// for why this file never guesses).
fn free_civil_build_move(
    r: &mut Replayer,
    actor: u8,
    rest: &str,
    built_card: CardId,
) -> Result<Option<Move>, MismatchKind> {
    let Some(using_pos) = rest.find(" using ") else {
        return Ok(None);
    };
    let after_using = &rest[using_pos + " using ".len()..];
    let Some((discount_card, _)) = longest_known_card_prefix(r.card_index, after_using) else {
        return Ok(None);
    };
    let grants_free_civil = discount_card
        .get()
        .special
        .iter()
        .any(|s| matches!(s, tta::cards::Special::FreeCivilAction(_)));
    if !grants_free_civil || !r.state.players[actor as usize].hand_civil.contains(discount_card) {
        return Ok(None);
    }
    r.try_apply(Move::PlayAction { card: discount_card })?;
    let Some(Pending::Choice(c)) = r.state.pending.top() else {
        return Err(MismatchKind::StuckPending(format!(
            "played {} for its free-civil-action discount but no Choice pending opened",
            discount_card.get().name
        )));
    };
    if !matches!(c.kind, ChoiceKind::FreeCivil { .. }) {
        return Err(MismatchKind::StuckPending(format!(
            "played {} but the pending choice is {:?}, not FreeCivil",
            discount_card.get().name,
            c.kind
        )));
    }
    let n = c
        .options
        .as_slice()
        .iter()
        .position(|o| matches!(o, ChoiceOption::Move(Move::Build { card }) if *card == built_card))
        .ok_or_else(|| {
            MismatchKind::ParserGap(format!(
                "{}'s free-civil-action options do not include building {}",
                discount_card.get().name,
                built_card.get().name
            ))
        })?;
    Ok(Some(Move::Choose { n: n as u8 }))
}

/// Translates one already-classified, already-actor-resolved journal line
/// into the `Move`(s) it represents and applies them. `rest` is the text
/// right after `"<Color> "` (what `tta::corpus::classify_after_actor`
/// itself dispatches on), reused here for the extra fields `classify`
/// doesn't surface.
fn apply_one(
    r: &mut Replayer,
    actor: u8,
    class: ActionClass,
    card: Option<CardId>,
    rest: &str,
    raw_text: &str,
    next_text: Option<&str>,
) -> Result<(), MismatchKind> {
    match class {
        ActionClass::TakeCard => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("take with no resolved card".into()))?;
            let cost = total_action_cost(raw_text);
            let slot = r
                .ground_row_slot(actor, card, cost)
                .ok_or_else(|| MismatchKind::ParserGap("no ungrounded row slot available to take from".into()))?;
            r.try_apply(Move::Take { slot })?;
            // The slot's REFILL (whatever `deal()` just drew into it) is
            // unobserved SIMULATED filler again -- ungroundeding it lets a
            // later take force the true next observed card into it, exactly
            // like a never-yet-touched slot. Without this, `row_grounded`
            // only ever grows, and once every low-cost slot has been taken
            // from once, later same-cost takes get force-placed into a
            // higher-cost slot purely because it's the only "fresh" one left
            // -- found by testing against a real 2p game (`docs/REPLAY.md`).
            r.row_grounded[slot as usize] = false;
            Ok(())
        }
        ActionClass::BuildBuilding | ActionClass::BuildUnit => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("build with no resolved card".into()))?;
            // An event's "each player with an unused worker may immediately
            // build X for free" left a `Pending::Choice(FreeBuild)` open for
            // `actor` (`resolve_intervening` stopped here on purpose rather
            // than guess which building without the parsed card -- see its
            // doc). If this build IS that free build, resolve it as a
            // `Choose`, not a priced `Move::Build`.
            if let Some(Pending::Choice(c)) = r.state.pending.top() {
                if matches!(c.kind, ChoiceKind::FreeBuild) {
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                        .ok_or_else(|| MismatchKind::ParserGap(format!("open FreeBuild choice does not offer {}", card.get().name)))?;
                    return r.try_apply(Move::Choose { n: n as u8 });
                }
            }
            // `"builds X using Y"`: Y is an Action card in hand printing
            // `freeCivilAction: build_or_upgrade_farm_or_mine` (Rich Land,
            // Urban Growth, ...) -- BGO folds "play Y" and "build X, for
            // free/discounted, as Y's effect" into one printed line the same
            // way it folds an action card's wonder-stage build into its
            // "plays ..." line (see `corpus.rs`'s
            // `plays_engineering_genius_...` test). The real move sequence
            // is `PlayAction{Y}` (opens `Pending::Choice(FreeCivil)`) then
            // `Choose{n}` for the option that IS `Move::Build{card}` --
            // never a bare `Move::Build`, which the engine plainly charges
            // full price (found by testing against a real 2p game;
            // `docs/REPLAY.md`).
            if let Some(mv) = free_civil_build_move(r, actor, rest, card)? {
                return r.try_apply(mv);
            }
            if let (Some(want), Some(got)) = (
                spent_resources(raw_text),
                costs::build_cost_for(&r.state, &r.state.players[actor as usize], card),
            ) {
                if want != got {
                    return Err(MismatchKind::UnrecoverableHiddenInfo(format!(
                        "build cost mismatch for {}: journal says {want} resources, this binary's \
                         reconstructed state computes {got} -- an unmodeled discount source (not a \
                         parser gap: the card and actor are both correctly resolved)",
                        card.get().name
                    )));
                }
            }
            r.try_apply(Move::Build { card })
        }
        ActionClass::BuildWonderStage => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("wonder stage with no resolved card".into()))?;
            let after_builds = rest.strip_prefix("builds ").ok_or_else(|| MismatchKind::ParserGap("wonder-stage line missing 'builds '".into()))?;
            let steps = wonder_stage_count(after_builds).ok_or_else(|| MismatchKind::ParserGap("could not parse wonder stage count".into()))?;
            let _ = card; // the wonder itself is implicit in state (under construction)
            r.try_apply(Move::WonderStep { steps })
        }
        ActionClass::IncreasePopulation => {
            let legal = legal::legal_moves(&r.state);
            if legal.as_slice().contains(&Move::Pop) {
                r.try_apply(Move::Pop)
            } else if legal.as_slice().contains(&Move::PopFree) {
                r.try_apply(Move::PopFree)
            } else {
                // Neither is legal -- almost always food/yellow-bank drift
                // from an earlier build/economy step this binary priced
                // differently than the true game (see the module doc's
                // "gives up on" list and `docs/REPLAY.md`'s mismatch
                // categories), not a parser gap in THIS line.
                Err(MismatchKind::IllegalMove {
                    attempted: "Pop or PopFree".into(),
                    legal_moves: format!("{:?}", legal.as_slice()),
                })
            }
        }
        ActionClass::UpgradeUnit | ActionClass::UpgradeProduction => {
            let to = card.ok_or_else(|| MismatchKind::ParserGap("upgrade with no resolved target card".into()))?;
            let after_upgrades = rest.strip_prefix("upgrades ").ok_or_else(|| MismatchKind::ParserGap("upgrade line missing 'upgrades '".into()))?;
            let from = upgrade_from_card(r.card_index, after_upgrades)
                .ok_or_else(|| MismatchKind::ParserGap("could not parse upgrade source card".into()))?;
            r.try_apply(Move::Upgrade { from, to })
        }
        ActionClass::DevelopTechnology => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("develop with no resolved card".into()))?;
            r.try_apply(Move::Develop { card })
        }
        ActionClass::ElectLeader => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("elect with no resolved card".into()))?;
            r.try_apply(Move::PlayLeader { card })
        }
        ActionClass::ChangeGovernment => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("revolution with no resolved card".into()))?;
            r.try_apply(Move::Revolution { card })
        }
        ActionClass::PlayActionCard => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("play-action with no resolved card".into()))?;
            r.try_apply(Move::PlayAction { card })
        }
        ActionClass::Destroy | ActionClass::Disband => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("destroy/disband with no resolved card".into()))?;
            if let Some(Pending::Choice(c)) = r.state.pending.top() {
                if matches!(c.kind, ChoiceKind::DestroyOwn) {
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                        .ok_or_else(|| MismatchKind::ParserGap("observed destroy card not among DestroyOwn options".into()))?;
                    return r.try_apply(Move::Choose { n: n as u8 });
                }
            }
            r.try_apply(Move::Destroy { card })
        }
        ActionClass::PlayTactic => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("tactic with no resolved card".into()))?;
            if rest.starts_with("adopts existing tactics ") {
                r.try_apply(Move::CopyTactic { card })
            } else {
                r.ground_military_hand(actor, card);
                r.try_apply(Move::PlayTactic { card })
            }
        }
        ActionClass::DeclareWar => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("war with no resolved card".into()))?;
            let target = color_after(rest, " on ").ok_or_else(|| MismatchKind::ParserGap("could not parse war target colour".into()))?;
            r.ground_military_hand(actor, card);
            r.try_apply(Move::War { card, target: target.seat() })
        }
        ActionClass::WinWar => Ok(()), // automatic (game::resolve_war_outcome); validation checkpoint only
        ActionClass::PlayAggression => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("aggression with no resolved card".into()))?;
            let target = color_after(rest, " against ").ok_or_else(|| MismatchKind::ParserGap("could not parse aggression target colour".into()))?;
            r.ground_military_hand(actor, card);
            r.try_apply(Move::Aggression { card, target: target.seat() })?;
            resolve_aggression_defense(r, next_text)
        }
        ActionClass::ProposePact => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("pact with no resolved card".into()))?;
            let target = color_after(rest, " to ").ok_or_else(|| MismatchKind::ParserGap("could not parse pact target colour".into()))?;
            let side = pact_side(raw_text, target_actor_color(actor), card);
            r.ground_military_hand(actor, card);
            r.try_apply(Move::OfferPact { card, target: target.seat(), side })
        }
        ActionClass::AcceptPact => {
            let Some(Pending::Choice(c)) = r.state.pending.top() else {
                return Err(MismatchKind::StuckPending("AcceptPact line but no pending PactOffer choice".into()));
            };
            if !matches!(c.kind, ChoiceKind::PactOffer { .. }) {
                return Err(MismatchKind::StuckPending("AcceptPact line but pending choice is not a PactOffer".into()));
            }
            let n = c
                .options
                .as_slice()
                .iter()
                .position(|o| matches!(o, ChoiceOption::Word(Keyword::Accept)))
                .ok_or_else(|| MismatchKind::ParserGap("PactOffer choice has no Accept option".into()))?;
            r.try_apply(Move::Choose { n: n as u8 })
        }
        ActionClass::Colonize => {
            // The auction/colonize sequence is driven entirely by the Bid/
            // BidPass lines and `resolve_intervening`'s auto-drain; by the
            // time a "colonizes a ..." line is reached it is a pure
            // validation checkpoint (nothing left to apply).
            let _ = card;
            Ok(())
        }
        ActionClass::Bid => {
            let n = rest
                .strip_prefix("bids ")
                .and_then(|s| s.trim_end_matches(|c: char| !c.is_ascii_digit() && !c.is_ascii_digit()).parse::<u8>().ok())
                .or_else(|| rest.strip_prefix("bids ").and_then(|s| s.split_whitespace().next()).and_then(|s| s.parse::<u8>().ok()))
                .ok_or_else(|| MismatchKind::ParserGap("could not parse bid amount".into()))?;
            r.try_apply(Move::Bid { n })
        }
        ActionClass::WinAuction => Ok(()), // automatic settlement of Pending::Auction; validation checkpoint only
        ActionClass::Pass => {
            if matches!(r.state.pending.top(), Some(Pending::Auction(_))) {
                r.try_apply(Move::BidPass)
            } else {
                r.try_apply(Move::PolPass)
            }
        }
        ActionClass::PlayEvent => Ok(()), // resolved automatically when the triggering PrepareEvent was inferred
        // The common case (an undo immediately following its own take) is
        // erased before this loop ever sees it -- `prescan_putback_skips`.
        // Reaching this arm means a `PutBack` with no matching preceding
        // `Take` of the same card by the same actor (a same-turn take/build/
        // take-back/rebuild sequence, or a parser gap); there is still no
        // engine `Move` for it.
        ActionClass::PutBack => Err(MismatchKind::UnrecoverableHiddenInfo(
            "unpaired BGO client-side undo (\"puts X back in the row\" with no matching preceding take)".into(),
        )),
        ActionClass::Discard => Err(MismatchKind::UnrecoverableHiddenInfo(
            "military hand discard: BGO logs only a count, never which card(s)".into(),
        )),
        ActionClass::EndTurn => r.try_apply(Move::EndTurn),
    }
}

/// After applying an `Aggression`, resolve the victim's `Pending::Defense`
/// using the count on the very next `"<Color> defends N ..."` bookkeeping
/// line, if any -- 0 is `DefendDone`; a positive count is unrecoverable
/// (identities never printed, see the module doc). If no `Pending::Defense`
/// opened at all (the victim had nothing eligible to spend), this is a
/// no-op. If the next line isn't a "defends" line at all, that means BGO
/// didn't log one for a defense that DID open -- treated as 0 committed
/// (the common case: RB, committing defense cards is rare and costly).
fn resolve_aggression_defense(r: &mut Replayer, next_text: Option<&str>) -> Result<(), MismatchKind> {
    if !matches!(r.state.pending.top(), Some(Pending::Defense(_))) {
        return Ok(());
    }
    match next_text.and_then(defends_count) {
        Some(n) if n > 0 => Err(MismatchKind::UnrecoverableHiddenInfo(format!(
            "aggression defense: {n} committed defense card(s), BGO logs only the count, never identities"
        ))),
        _ => r.try_apply(Move::DefendDone),
    }
}

/// `color_after`'s callers need the ACTOR's own colour (not the target's)
/// to build the `"<Actor> is A"` marker `pact_side` searches for.
fn target_actor_color(seat: u8) -> Color {
    match seat {
        0 => Color::Orange,
        1 => Color::Purple,
        2 => Color::Green,
        _ => Color::Grey,
    }
}

// ---------------------------------------------------------------------
// main
// ---------------------------------------------------------------------

fn run(index_path: &str, journals_dir: &str, ids: &[String]) -> Result<(), String> {
    let card_index = build_card_index();
    let games = corpus::parse_index(index_path)?;
    let by_id: HashMap<&str, &GameMeta> = games.iter().map(|g| (g.id.as_str(), g)).collect();

    let mut n_completed = 0usize;
    let mut n_score_match = 0usize;
    let mut n_score_checked = 0usize;
    let mut n_approx = 0usize;

    for id in ids {
        let Some(meta) = by_id.get(id.as_str()) else {
            println!("{id}: not found in index.tsv");
            continue;
        };
        let path = format!("{journals_dir}/{id}.tsv");
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                println!("{id}: no journal file ({e})");
                continue;
            }
        };
        let result = replay_game(meta, &text, &card_index);
        print_result(&result);
        if result.completed {
            n_completed += 1;
            if result.colonize_approximated {
                n_approx += 1;
            }
            if let Some(engine) = &result.engine_scores {
                n_score_checked += 1;
                let mut a = engine.clone();
                let mut b = result.index_scores.clone();
                a.sort_unstable();
                b.sort_unstable();
                if a == b {
                    n_score_match += 1;
                }
            }
        }
    }

    println!(
        "\n{n_completed}/{} games replayed to completion with every human action legal ({n_approx} used the colonize approximation).",
        ids.len()
    );
    println!("{n_score_match}/{n_score_checked} completed games' final scores matched index.tsv (sorted multiset comparison).");
    Ok(())
}

fn print_result(g: &GameResult) {
    let status = if g.completed { "COMPLETE" } else { "STOPPED" };
    print!("{} [{}p] {status} after {} actions", g.id, g.players, g.actions_consumed);
    if g.colonize_approximated {
        print!(" (colonize approximated)");
    }
    if let Some(engine) = &g.engine_scores {
        let mut a = engine.clone();
        let mut b = g.index_scores.clone();
        a.sort_unstable();
        b.sort_unstable();
        print!(" scores engine={engine:?} index={:?} match={}", g.index_scores, a == b);
    }
    println!();
    if let Some(m) = &g.mismatch {
        println!(
            "    line {} (age {} round {}): {}",
            m.lineno, m.age, m.round, m.raw_text
        );
        println!("    -> {:?}", m.kind);
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    if argv.len() < 3 {
        eprintln!("usage: replay <index.tsv> <journals_dir> <game_id> [game_id ...]");
        return ExitCode::FAILURE;
    }
    match run(&argv[0], &argv[1], &argv[2..]) {
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

    #[test]
    fn total_action_cost_sums_a_civil_and_a_military_clause_on_one_line() {
        // Hammurabi's leader ability converts part of a take's cost from a
        // civil action to a military one -- the two clauses share a line.
        let text = "Orange takes Breakthrough in hand Orange uses 1 civil action; \
                     Orange uses 1 military action";
        assert_eq!(total_action_cost(text), Some(2));
    }

    #[test]
    fn total_action_cost_reads_a_single_civil_clause() {
        let text = "Orange takes Engineering Genius in hand Orange uses 1 civil action";
        assert_eq!(total_action_cost(text), Some(1));
    }

    #[test]
    fn total_action_cost_is_none_when_no_uses_clause_is_present() {
        assert_eq!(total_action_cost("Orange increases population Orange spends 2 food"), None);
    }

    #[test]
    fn color_after_finds_the_war_target_past_the_on_marker() {
        let text = "declares War over Culture on Green The victor takes 5 culture ...";
        assert_eq!(color_after(text, " on "), Some(Color::Green));
    }

    #[test]
    fn color_after_finds_the_aggression_target_past_the_against_marker() {
        let text = "plays Plunder against Orange Your rival loses ...";
        assert_eq!(color_after(text, " against "), Some(Color::Orange));
    }

    #[test]
    fn color_after_is_none_when_the_marker_is_not_followed_by_a_known_colour() {
        assert_eq!(color_after("declares War over Culture on nobody", " on "), None);
    }

    #[test]
    fn wonder_stage_count_reads_the_leading_digit() {
        assert_eq!(wonder_stage_count("1 stage of Pyramids; ; Wonder completed"), Some(1));
        assert_eq!(wonder_stage_count("2 stages of Colossus"), Some(2));
    }

    #[test]
    fn spent_resources_reads_a_resources_clause_not_a_food_clause() {
        assert_eq!(spent_resources("Purple builds Bronze Purple spends 2 resources"), Some(2));
        assert_eq!(spent_resources("Purple increases population Purple spends 1 food"), None);
    }

    #[test]
    fn spent_resources_reads_the_discounted_amount_on_a_using_line() {
        assert_eq!(
            spent_resources("Purple builds Printing Press using Urban Growth Purple spends 2 resources"),
            Some(2)
        );
    }

    #[test]
    fn defends_count_reads_the_committed_card_count() {
        let text = "Orange defends 1 Defense card +6 played; Orange strength: 26; Purple strength: 26";
        assert_eq!(defends_count(text), Some(1));
    }

    #[test]
    fn defends_count_is_none_for_a_line_that_is_not_a_defends_line() {
        assert_eq!(defends_count("Purple builds Bronze Purple spends 2 resources"), None);
    }

    #[test]
    fn current_event_name_extracts_the_bare_name_between_the_slash_and_the_semicolon() {
        let text = "Purple plays event Purple scores 1 culture; Current event:; \
                     A / Development of Agriculture; Each civilization gains 2 food.";
        assert_eq!(current_event_name(text), Some("Development of Agriculture"));
    }

    #[test]
    fn current_event_name_is_none_when_there_is_no_current_event_clause() {
        assert_eq!(current_event_name("Orange builds Bronze Orange spends 2 resources"), None);
    }
}
