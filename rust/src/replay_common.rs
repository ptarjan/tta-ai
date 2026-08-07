//! The game-state reconstruction machinery behind `bin/replay.rs`
//! (`docs/REPLAY.md`), extracted into the library so `bin/agreement.rs`
//! (`docs/REPLAY.md`'s planned move-agreement analysis) can reuse it without
//! duplicating it -- `bin/*.rs` files are separate crates that cannot import
//! from one another directly, only from this library, which is the entire
//! reason this module exists apart from `bin/replay.rs` itself.
//! `bin/replay.rs` is now a thin CLI wrapper (`run`/`print_result`/`main`)
//! over [`replay_game`]; see its own doc comment for the exact invocation.
//!
//! For a given BGO human game id, walks `sources/bgo/journals/<id>.tsv` in
//! order, translates each line into the corresponding engine [`Move`], and
//! applies it through the REAL engine (`legal::legal_moves`, `apply::apply`)
//! -- never by hand-mutating `GameState` to force a match. At every step the
//! human's action must appear in `legal_moves()` for the reconstructed
//! state; when it does not, that is recorded as a structured [`Mismatch`]
//! and the game stops there.
//!
//! # The `Decision` recording hook, for `agreement.rs`
//!
//! [`Replayer::try_apply`] is the single choke point through which every
//! journal-observed HUMAN move (as opposed to an auto-resolution this file
//! infers on the human's behalf -- a stale pending drain, an inferred hidden
//! `PrepareEvent`, the colonize auto-drain, a forced political pass) is
//! checked against `legal_moves` and applied. Call sites that translate a
//! real, journal-observed human action pass `record: true`; internal
//! auto-resolutions (and moves with no real strategic content, like the
//! forced 0-defender `DefendDone`) pass `false`. When [`Replayer::
//! record_decisions`] is set (via [`replay_game`]'s own `record_decisions`
//! parameter -- `false`, i.e. zero-cost, for `replay`'s own binary), each
//! `record: true` call snapshots the PRE-move `GameState`, the exact
//! `legal_moves` list, and the human's chosen `Move` into [`Replayer::
//! decisions`], returned from `replay_game` as [`GameResult::decisions`].
//! `GameState::clone` is a flat, alloc-free structural copy (`bots/mod.rs`'s
//! own doc comment), so snapshotting a few hundred of these per game is
//! cheap -- no closures, no trait objects, no bot dependency in this module
//! at all: what a caller does with the snapshot (rank it with a bot, or
//! anything else) is entirely up to them.
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
//! # Event/Territory preparation: solved from the journal, not inferred
//!
//! `Move::PrepareEvent` (the political action that puts an Event or
//! Territory card on the future-events pile and reveals the top of the
//! current-events pile) used to be treated here as unrecoverable hidden
//! information: this file GUESSED forward, inferring a preparation at every
//! Politics decision no journal line explained, and popping the next
//! observed reveal off a FIFO to satisfy it. That premise was wrong, and one
//! wrong guess (a player who simply passed) desynchronised every event after
//! it.
//!
//! BGO logs every preparation, as one line:
//!
//! ```text
//! Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; ...
//! ```
//!
//! which names the preparer (the line's actor), the AGE of the card they
//! prepared (`apply::h_prepare_event` scores exactly `card.level()`), and
//! what the reveal turned up. [`crate::event_plan`] solves the whole game's
//! record from those lines before the first line is replayed -- including
//! which specific card each preparation put on the pile, pinned by the
//! constraint that each pile IS the set of cards prepared into the previous
//! one. See that module's doc for the constraint, its corpus-wide
//! verification, and the one thing that stays underdetermined.
//!
//! What this file then does per decision is not an inference at all:
//! `resolve_political_decision` consumes the next solved preparation when it
//! belongs to this decider and its line has been reached, and otherwise
//! applies `Move::PolPass`. The setup pile and every recycle are grounded to
//! the journal's own reveal order (`set_current_events`) -- the pile
//! CONTENTS come from the engine, only the never-logged shuffle order is
//! replaced -- so the "is the right card on top?" check before each
//! preparation is a real test of the model rather than a re-forcing of the
//! answer, and it stops the game (`MismatchKind::EventPlanInfeasible`) when
//! it fails.
//!
//! # What this file gives up on, and why
//!
//! - **Discard** (§6.6 hand-limit, and any other forced military discard):
//!   BGO's journal logs only a count (`"<Color> discards N cards"`), never
//!   which cards. NOT given up on: `discard_solver::DiscardSolver` resolves
//!   this by constraint propagation over the rest of the journal -- see that
//!   module's doc and `docs/REPLAY.md`'s "Military discard: solved, not
//!   given up on" section for the full argument and the honest solved-vs-
//!   chosen-vs-forced-collision accounting.
//! - **Aggression defense** used to be listed here as unrecoverable ("BGO
//!   logs only a count, never which cards") -- that was false, found by
//!   reading the raw clauses instead of trusting this comment. BGO's
//!   `"<Color> defends ..."` line is not one bare count; it is one clause
//!   PER committed card. A `"Defense card +<n> played"` clause names its
//!   printed bonus (2/4/6) directly, and `data/cards_military_actions.json`
//!   has exactly one `bonus`-type card per value (one per age I/II/III), so
//!   the number alone is the card's full identity -- the six physical
//!   copies per age are interchangeable, so "which of the six" was never a
//!   real question. A `"military card played"` clause is any hand card
//!   whose `defense_bonus` is 0 (`interact::defense_points`'s flat +1
//!   branch); resolved via `discard_solver::DiscardSolver` exactly like a
//!   forced hand-limit discard, because it is the same fact (a specific
//!   card permanently leaves the hand) with the same kind of residual,
//!   honestly-counted ambiguity. See [`resolve_aggression_defense`].
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

use crate::corpus::{
    actor_and_rest, best_age_sibling, classify, family_siblings, longest_known_card_prefix, ActionClass, Classified,
    Color, GameMeta, LineOutcome,
};
pub use crate::corpus::build_card_index;
use crate::discard_solver::{DiscardSolver, FutureNeed};
use crate::event_plan::EventPlan;
use crate::moves::PactSide;
use crate::state::{Choice, ChoiceKind, ChoiceOption, GameState, Keyword, Pending, PlayerState, Phase};
use crate::{apply, costs, economy, game, legal, CardId, CardType, Move};

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
pub enum MismatchKind {
    /// A hidden piece of state (BGO's client-side undo, an unmodeled build
    /// discount) genuinely cannot be recovered from the journal; see this
    /// file's module doc.
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
    /// The journal's own event record could not be reconciled with the
    /// events pile model -- see [`crate::event_plan`]. Its own doc explains
    /// why this is the interesting failure: the constraint holds in every
    /// one of the corpus's 3,291 pile windows, so a violation means one of
    /// the models around it is wrong, not that the solve needs loosening.
    EventPlanInfeasible(String),
}

pub struct Mismatch {
    pub lineno: usize,
    pub age: String,
    pub round: String,
    pub raw_text: String,
    pub kind: MismatchKind,
}

// ---------------------------------------------------------------------
// Decision recording, for `agreement.rs` -- see the module doc's own
// section on this.
// ---------------------------------------------------------------------

/// One point in a replayed game where this file is about to apply the
/// HUMAN's own journal-observed move against the legal-move list a bot could
/// equally have been asked to rank -- see [`Replayer::try_apply`]'s `record`
/// parameter for exactly which call sites this fires at (and which don't).
pub struct Decision {
    /// The journal line this decision was translated from (`Line::lineno`) --
    /// enough to go back and print the surrounding raw journal text for a
    /// specific disagreement later.
    pub lineno: usize,
    /// The PRE-move `GameState` -- the same position a bot asked to play
    /// this decision would see. A flat, alloc-free `Clone` (see the module
    /// doc), so cloning one per decision point is cheap.
    pub state: GameState,
    /// Exactly what `legal::legal_moves(&state)` returned at this point --
    /// the same candidate list `human_move` was drawn from.
    pub legal_moves: Vec<Move>,
    /// The move this file constructed from the journal line and is about to
    /// apply -- always a member of `legal_moves`, since `try_apply` only
    /// records after its own legality check has already passed.
    pub human_move: Move,
    /// Whether an earlier `Pending::Choice(DiscardMilitary)` THIS game has
    /// already resolved was an arbitrary pick (`DiscardSolver`'s "chosen" or
    /// "forced collision" buckets, never "solved") rather than a
    /// constraint-derived certainty -- see `discard_solver`'s module doc.
    /// Every card identity downstream of such a discard is grounded in a
    /// guess, not a fact, so a decision point reached after one is slightly
    /// fictional (`docs/REPLAY.md`'s own caveat) -- flagged here rather than
    /// silently mixed in with clean decision points, so a later analysis can
    /// filter on it.
    pub after_arbitrary_discard: bool,
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
    /// Every event preparation in the game -- who made it, which age of card
    /// they put on the future-events pile, and what their reveal turned up --
    /// solved once per game from the journal by [`crate::event_plan::solve`].
    /// See the module doc's "Event/Territory preparation" section.
    plan: EventPlan,
    /// Index into `plan.preparations` of the next one still to be applied.
    next_prep: usize,
    /// Per-seat count of `Move::PolPass`es this file applied on the player's
    /// behalf during their CURRENT turn that the journal has not yet caught
    /// up to. BGO logs a declined political action (`"<Color> passes
    /// Political Phase"`) at the point the human clicked it, which for
    /// Julius Caesar's offered-and-declined SECOND action is routinely after
    /// some of that player's own Action-phase lines -- the engine has to
    /// close the politics phase before any of those are legal, so the pass
    /// is applied first and its journal line arrives later as a pure
    /// confirmation. Reset when the player's turn ends.
    auto_passed: [u32; 4],
    /// Whether any colonization in this game was resolved by the
    /// approximate auto-drain rather than a verified sacrifice match.
    colonize_approximated: bool,
    /// Number of actionable (non-bookkeeping) journal lines consumed.
    actions_consumed: usize,
    /// The journal `Line::lineno` currently being resolved, set once per
    /// loop iteration in `replay_game` before `resolve_intervening` runs.
    /// `DiscardSolver::choose` needs this to tell a FUTURE named play
    /// (still in hand right now, so not a valid discard candidate) from a
    /// PAST one (already left the hand, so it isn't excluded) -- see that
    /// module's doc. `0` (no line has "already happened") is a safe initial
    /// value: every real journal line number is >= 2 (`parse_lines` skips
    /// the header).
    current_lineno: usize,
    /// Per-seat FIFO of `(is_resources, amount)` pulled off every
    /// standalone `"<Color> produces N food"` / `"<Color> produces N
    /// resources"` bookkeeping line, pre-scanned once per game -- see
    /// `resolve_intervening`'s `ChoiceKind::GainBlock` handling and
    /// `prescan_gain_produces`'s doc comment.
    gain_produces: HashMap<u8, VecDeque<(bool, i32)>>,
    /// Resolves `Pending::Choice(DiscardMilitary)` by constraint propagation
    /// over the rest of the journal -- see `discard_solver`'s module doc and
    /// `docs/REPLAY.md`'s "Military discard: solved, not given up on"
    /// section. Also tallies the solved/chosen/forced-collision counts this
    /// game's replay reports.
    discard_solver: DiscardSolver,
    /// Whether [`Replayer::try_apply`]'s `record: true` call sites should
    /// snapshot a [`Decision`] into `decisions` -- set once, right after
    /// construction, by [`replay_game`]'s own `record_decisions` parameter.
    /// `false` (the default via `Replayer::new`) costs nothing beyond the
    /// branch itself: no clone, no allocation.
    record_decisions: bool,
    /// Accumulated in journal order by every `record: true` call to
    /// `try_apply` while `record_decisions` is set -- see the module doc's
    /// "Decision recording" section. Drained into [`GameResult::decisions`]
    /// at the end of [`replay_game`].
    decisions: Vec<Decision>,
}

/// Overwrite the current-events pile with `reveal_order` -- the journal's
/// own order for the cards that pile is going to turn up. The pile is
/// popped from the END (`events::reveal_current_event`), so the first card
/// to be revealed has to sit last. This is GROUNDING in the module doc's
/// sense, and the only kind of state this file writes by hand: the pile's
/// contents come from the engine (the setup deal, or `recycle_future_events`
/// moving the future pile over), and only the shuffle ORDER -- fictional in
/// a reconstruction, since the real one was never logged -- is replaced by
/// the observed one.
fn set_current_events(state: &mut GameState, reveal_order: &[CardId]) {
    state.current_events = crate::state::CardList::new();
    for &card in reveal_order.iter().rev() {
        state.current_events.push(card);
    }
    crate::events::sync_current_events_age(state);
}

impl<'a> Replayer<'a> {
    fn new(
        card_index: &'a HashMap<&'static str, CardId>,
        num_players: u8,
        plan: EventPlan,
        gain_produces: HashMap<u8, VecDeque<(bool, i32)>>,
        future_military_needs: HashMap<u8, Vec<FutureNeed>>,
    ) -> Self {
        // The seed is thrown away semantically -- every field it determines
        // (deck order, starting row/hand contents) is SIMULATED filler this
        // binary overwrites the instant a slot/hand entry is observed. It is
        // fixed (not random) purely so a run is reproducible byte-for-byte.
        let mut state = game::new_game(num_players, 0xC0FFEE);
        // The one exception, grounded up front rather than lazily: the
        // setup current-events pile. `new_game` deals `num_players + 2`
        // RANDOM Age A cards there; the journal names every one it ever
        // turns up, in order, so the real pile is known before the first
        // line is read. `current_events` is popped from the END, so reveal
        // order is stored reversed.
        set_current_events(&mut state, &plan.setup_pile);
        Replayer {
            card_index,
            state,
            row_grounded: [false; 13],
            plan,
            next_prep: 0,
            auto_passed: [0; 4],
            colonize_approximated: false,
            actions_consumed: 0,
            current_lineno: 0,
            gain_produces,
            discard_solver: DiscardSolver::new(future_military_needs),
            record_decisions: false,
            decisions: Vec::new(),
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
            if std::env::var("REPLAY_DEBUG_ALL").is_ok() {
                eprintln!(
                    "DEBUG resolve_intervening loop: decider={decider} expected_actor={expected_actor} upcoming={upcoming:?} pending_top={:?}",
                    self.state.pending.top()
                );
            }
            if let Some(Pending::Choice(c)) = self.state.pending.top().cloned() {
                // A `Pending::Choice(GainBlock)` (an event's "Each
                // civilization gains 2 resources or 2 food (player's
                // choice)" -- e.g. "Development of Markets") is opened for
                // EVERY player, one choice each, the instant the event
                // resolves -- not just the player who played it. BGO logs
                // each player's own pick as its OWN standalone bookkeeping
                // line, `"<Color> produces N food"` / `"<Color> produces N
                // resources"` (`corpus.rs` already treats this shape as
                // bookkeeping -- not a distinct action -- for census
                // purposes; `prescan_gain_produces` below re-reads it here
                // because a real `Move::Choose` is still needed to clear the
                // pending). Like `FreeBuild` below, this is drained
                // regardless of whose turn it nominally is (it blocks ANY
                // further action by that player, including their own, until
                // resolved) -- found by testing against a real 3p game
                // where a `WonderStep` several lines later was rejected
                // because the actor's own still-open choice from an EARLIER
                // event had never been cleared (`docs/REPLAY.md`).
                if matches!(c.kind, ChoiceKind::GainBlock) {
                    let picked = self
                        .gain_produces
                        .get_mut(&decider)
                        .and_then(|q| q.pop_front())
                        .ok_or_else(|| {
                            MismatchKind::StuckPending(format!(
                                "GainBlock choice open for player {decider} but no journal-observed \
                                 \"produces\" line left to resolve it with"
                            ))
                        })?;
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| match o {
                            ChoiceOption::Gain(g) => {
                                (picked.0 && g.resources as i32 == picked.1 && picked.1 != 0)
                                    || (!picked.0 && g.food as i32 == picked.1 && picked.1 != 0)
                            }
                            _ => false,
                        })
                        .ok_or_else(|| {
                            MismatchKind::ParserGap(format!(
                                "GainBlock options {:?} do not offer the journal-observed {} {}",
                                c.options.as_slice(),
                                picked.1,
                                if picked.0 { "resources" } else { "food" }
                            ))
                        })?;
                    apply::apply(&mut self.state, Move::Choose { n: n as u8 });
                    continue;
                }
                // A `Pending::Choice(FreeBuild)` (an event's "each player
                // with an unused worker may immediately build X for free"
                // -- e.g. "Development of Religion") is left open regardless
                // of WHOSE turn it nominally is, is not gated on `phase`,
                // and a human DECLINING it (`Skip`) leaves no journal trace
                // at all -- the same silent-response shape as a
                // Politics-phase pass, just for a different pending kind.
                // Drained here, ahead of the decider-equality check below,
                // exactly like the Politics case: if the upcoming line is a
                // build that matches one of its options, stop here and let
                // `apply_one`'s Build handling resolve it (it needs the
                // parsed card, which this function doesn't have reason to
                // duplicate); otherwise assume `Skip` and keep draining
                // (there can be one such pending per qualifying player,
                // queued back to back) -- found by testing against a real
                // 3p game (`docs/REPLAY.md`).
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
                // A `Pending::Choice(DiscardMilitary)` (`interact::
                // discard_excess_military`, `apply.rs`'s implementation of
                // §6.6 step 1) blocks EVERY further action by the player it
                // is open for -- their own next move AND, if they are not
                // `decider`, whoever else is trying to act while it sits
                // unresolved (the "reached through a different code path"
                // shape `docs/REPLAY.md` used to report as unrecoverable).
                // Two cases:
                //
                // - This IS `expected_actor`'s own pending, and the line
                //   about to be translated is their own `"discards N
                //   cards"` line (`upcoming.0 == Discard`): defer to
                //   `apply_one`'s `Discard` arm (via `resolve_one_discard`),
                //   exactly like the `FreeBuild` `matches_upcoming` case
                //   above -- NOT because it needs the parsed line (unlike
                //   `FreeBuild` it doesn't), but because resolving the LAST
                //   queued discard can itself finish that player's end of
                //   turn and advance `state.current` to the NEXT player
                //   (`interact::QueueItem::DiscardMilitary`'s own doc:
                //   resolving it resumes `game::resume_end_turn`) -- doing
                //   that HERE, before the decider-equality check below,
                //   would make `decider` legitimately stop matching
                //   `expected_actor` and get wrongly reported as a stuck
                //   pending, even though the line was fully, correctly
                //   consumed. Found by testing against real games where
                //   EVERY "discards" line failed this way the moment this
                //   branch existed unconditionally.
                // - Anything else (a DIFFERENT player's stale discard, or
                //   this player's discard reached from an unrelated line --
                //   the original "different code path" shape): drain it
                //   here, same as `GainBlock`/`FreeBuild`, since nothing
                //   else in this file will ever get a chance to.
                if matches!(c.kind, ChoiceKind::DiscardMilitary) {
                    let matches_upcoming = c.player == expected_actor && upcoming.0 == ActionClass::Discard;
                    if matches_upcoming {
                        return Ok(());
                    }
                    self.resolve_one_discard_choice(&c);
                    continue;
                }
            }
            if decider == expected_actor {
                let own_politics_decision = self.state.phase == Phase::Politics && self.state.pending.is_empty();
                // A preparation whose own line is already behind us outranks
                // the line about to be translated, even when that line is
                // this player's own explicit political action: BGO's
                // `"plays event"` line is skipped as a confirmation
                // (`is_pure_confirmation_line`), so the preparation it
                // records is applied here, at the next line -- and Julius
                // Caesar's declined SECOND action makes that next line
                // routinely the player's own `"passes Political Phase"`.
                // Without this the pass would be applied as the FIRST
                // political action and the preparation would be stranded.
                let owed_preparation = own_politics_decision && self.claimable_preparation(decider).is_some();
                if !own_politics_decision || (next_line_explains_own_politics && !owed_preparation) {
                    return Ok(());
                }
                self.resolve_political_decision(decider)?;
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
                        self.resolve_political_decision(decider)?;
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

    /// The next solved preparation, if it is `decider`'s and the journal
    /// line it was read off has already been reached. `None` means this
    /// player's political decision, whatever it was, was not a preparation.
    fn claimable_preparation(&self, decider: u8) -> Option<crate::event_plan::Preparation> {
        self.plan
            .preparations
            .get(self.next_prep)
            .filter(|p| p.actor == decider && p.lineno <= self.current_lineno)
            .copied()
    }

    /// Resolve a Politics-phase decision for `decider` that no journal line
    /// has explicitly claimed. There are exactly two possibilities and the
    /// journal distinguishes them outright, so nothing here is guessed:
    /// either this player's next `"plays event"` line has already gone by
    /// (they PREPARED an event -- see [`crate::event_plan`], which solved
    /// who, when, and which card before the first line was read), or they
    /// passed, which BGO logs sometimes and omits sometimes.
    ///
    /// The preparation queue is consumed strictly in journal order. The
    /// front entry is claimed only when it belongs to `decider` AND its own
    /// line has already been reached (`lineno <= current_lineno`) -- the
    /// second half is what keeps a player's LATER preparation from being
    /// pulled forward onto an EARLIER turn on which they really did pass,
    /// which is precisely the failure the old forward guess made.
    fn resolve_political_decision(&mut self, decider: u8) -> Result<(), MismatchKind> {
        let claimed = self.claimable_preparation(decider);
        if std::env::var("REPLAY_DEBUG_ALL").is_ok() {
            eprintln!(
                "DEBUG resolve_political_decision decider={decider} round={} lineno={} claims={claimed:?}",
                self.state.round, self.current_lineno,
            );
        }
        let Some(prep) = claimed else {
            self.auto_passed[decider as usize] += 1;
            return self.try_apply(Move::PolPass, false);
        };
        self.next_prep += 1;

        // The card the pile is about to turn up must already be on top of
        // it -- the setup pile and every recycle are grounded from the
        // journal, so this is a real check on the pile model, not a
        // re-forcing of the answer.
        let on_top = self.state.current_events.as_slice().last().copied();
        if on_top != Some(prep.revealed) {
            return Err(MismatchKind::EventPlanInfeasible(format!(
                "line {}: the journal reveals {:?} here, but the current-events pile has {:?} on top",
                prep.lineno,
                prep.revealed.get().name,
                on_top.map(|c| c.get().name),
            )));
        }

        self.state.players[decider as usize].hand_military.push(prep.card);
        let mv = Move::PrepareEvent { card: prep.card };
        let legal = legal::legal_moves(&self.state);
        if !legal.as_slice().contains(&mv) {
            return Err(MismatchKind::IllegalMove {
                attempted: format!("{mv:?} (journal-observed preparation by player {decider}, line {})", prep.lineno),
                legal_moves: format!("{:?}", legal.as_slice()),
            });
        }
        apply::apply(&mut self.state, mv);

        // This reveal emptied the pile, so `reveal_current_event` has
        // already recycled the future pile into it -- with the right CARDS
        // (they are the ones `event_plan` solved) but a fictional shuffle
        // order. Replace that order with the journal's own.
        if prep.ends_batch {
            let next = self.plan.next_batch_reveals(self.next_prep - 1);
            // Multiset difference, not a set one: a deck can hold two
            // copies of the same card.
            let mut leftover: Vec<CardId> = self.state.current_events.as_slice().to_vec();
            for card in &next {
                let Some(at) = leftover.iter().position(|c| c == card) else {
                    return Err(MismatchKind::EventPlanInfeasible(format!(
                        "line {}: the journal's next pile turns up {:?}, which the engine's recycled pile {:?} \
                         does not contain",
                        prep.lineno,
                        card.get().name,
                        self.state.current_events.as_slice().iter().map(|c| c.get().name).collect::<Vec<_>>(),
                    )));
                };
                leftover.swap_remove(at);
            }
            // Cards the game ended before revealing are never popped, so
            // they go under the observed ones and their order is arbitrary.
            let mut order = next;
            order.extend_from_slice(&leftover);
            set_current_events(&mut self.state, &order);
        }

        if std::env::var("REPLAY_DEBUG_ALL").is_ok() {
            eprintln!(
                "DEBUG PrepareEvent applied for decider={decider} prepared={:?} revealed={:?} -> pending_top={:?} current_events={:?} future_events={:?}",
                prep.card.get().name,
                prep.revealed.get().name,
                self.state.pending.top(),
                self.state.current_events.as_slice(),
                self.state.future_events.as_slice(),
            );
        }
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

    /// Resolve exactly the CURRENTLY open `Pending::Choice(DiscardMilitary)`
    /// -- `c` must be that pending's own snapshot, read by the caller just
    /// before calling this (`resolve_intervening` and `resolve_discard`
    /// both do). Applying the `Move::Choose` this picks may itself finish
    /// the discarding player's end of turn and advance `state.current` --
    /// see `resolve_intervening`'s `DiscardMilitary` branch for why that
    /// matters to callers.
    fn resolve_one_discard_choice(&mut self, c: &Choice) {
        let opts: Vec<CardId> = c
            .options
            .as_slice()
            .iter()
            .map(|o| match o {
                ChoiceOption::Card(id) => *id,
                other => panic!("DiscardMilitary choice offered a non-card option {other:?}"),
            })
            .collect();
        let (n, certainty) = self.discard_solver.choose(c.player, self.current_lineno, &opts);
        if std::env::var("REPLAY_DEBUG_ALL").is_ok() {
            eprintln!(
                "DEBUG discard: player {} line {} picked {} of {} candidates ({certainty:?})",
                c.player,
                self.current_lineno,
                opts[n].get().name,
                opts.len()
            );
        }
        apply::apply(&mut self.state, Move::Choose { n: n as u8 });
    }

    /// Drain every currently open `Pending::Choice(DiscardMilitary)`
    /// belonging to `actor` -- called from `apply_one`'s `ActionClass::
    /// Discard` arm, i.e. when the journal line being translated IS that
    /// player's own `"discards N cards"` line. Stops the instant the top
    /// pending is no longer a `DiscardMilitary` choice for `actor`: either
    /// it fully resolved (the common case), or -- reaching a DIFFERENT
    /// pending or a different player's discard here would mean this
    /// binary's own reconstructed hand size disagrees with the journal's
    /// stated count, which is a real, separate mismatch this function
    /// deliberately does not paper over by draining someone else's pending
    /// to make the count match.
    fn resolve_discard(&mut self, actor: u8) {
        while let Some(Pending::Choice(c)) = self.state.pending.top().cloned() {
            if c.kind != ChoiceKind::DiscardMilitary || c.player != actor {
                break;
            }
            self.resolve_one_discard_choice(&c);
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
            // The journal told us exactly what this take cost, and NO
            // available slot reproduces it under this binary's own cost
            // formula (`costs::take_cost`) -- placing the card in whichever
            // slot happens to be cheapest anyway (the old behaviour here)
            // silently commits to a WRONG cost, which then reliably shows up
            // several lines later as a much harder to diagnose "budget
            // shortfall" `IllegalMove` once the difference compounds
            // (confirmed against a real 2p game, `7523353`: a Wonder take
            // this binary priced 1 CA too high, because a same-turn-
            // completed wonder's take-surcharge did not match the human's
            // OWN paid cost -- see `docs/REPLAY.md`'s open questions).
            // Failing HERE instead is the honest report: a genuine cost
            // disagreement between this binary's model and the observed
            // journal, not a slot-placement guess.
            return None;
        }
        let i = *ungrounded.first()?;
        self.state.card_row[i] = card;
        self.row_grounded[i] = true;
        Some(i as u8)
    }

    /// Apply `mv` if legal; otherwise build an `IllegalMove` mismatch.
    /// Check `mv` against `legal_moves` and apply it if legal.
    ///
    /// `record` distinguishes a real, journal-observed HUMAN move (`true`)
    /// from an internal auto-resolution this file infers on the human's
    /// behalf (`false` -- a stale-pending drain, an inferred hidden
    /// `PrepareEvent`, the forced 0-defender `DefendDone`, ...): see the
    /// module doc's "Decision recording" section. Every call site in this
    /// file passes an explicit, considered value -- there is no default,
    /// on purpose, so a new call site can't silently mis-tag itself either
    /// way.
    fn try_apply(&mut self, mv: Move, record: bool) -> Result<(), MismatchKind> {
        let legal = legal::legal_moves(&self.state);
        if !legal.as_slice().contains(&mv) {
            if std::env::var("REPLAY_DEBUG").is_ok() {
                let p = &self.state.players[self.state.current as usize];
                eprintln!(
                    "DEBUG try_apply fail: mv={mv:?} actor(current)={} civil_actions={} military_actions={} government={} leader={} phase={:?} pending_top={:?} hand_civil_size={} civil_hand_limit={} hand_civil={:?} resources={} food={} mil_discount={} card_row={:?}",
                    self.state.current,
                    p.civil_actions,
                    p.military_actions,
                    p.government.get().name,
                    if p.leader.is_none() { "none" } else { p.leader.get().name },
                    self.state.phase,
                    self.state.pending.top(),
                    p.hand_size_civil(),
                    costs::civil_hand_limit(&self.state, p),
                    p.hand_civil.as_slice().iter().map(|id| id.get().name).collect::<Vec<_>>(),
                    p.resources,
                    p.food,
                    p.mil_discount,
                    (0..13).map(|i| if self.state.card_row[i].is_none() { "-".to_string() } else { self.state.card_row[i].get().name.to_string() }).collect::<Vec<_>>(),
                );
            }
            return Err(MismatchKind::IllegalMove {
                attempted: format!("{mv:?}"),
                legal_moves: format!("{:?}", legal.as_slice()),
            });
        }
        if record && self.record_decisions {
            // Never "solved" -- a constraint-derived certainty is a real
            // fact, not a guess -- so only "chosen"/"forced_collision" taint
            // downstream decisions. See `Decision::after_arbitrary_discard`.
            let after_arbitrary_discard =
                self.discard_solver.chosen > 0 || self.discard_solver.forced_collisions > 0;
            self.decisions.push(Decision {
                lineno: self.current_lineno,
                state: self.state.clone(),
                legal_moves: legal.as_slice().to_vec(),
                human_move: mv,
                after_arbitrary_discard,
            });
        }
        let pending_top_before = self.state.pending.top().cloned();
        apply::apply(&mut self.state, mv);
        if std::env::var("REPLAY_DEBUG_ALL").is_ok() {
            let p = &self.state.players[self.state.current as usize];
            eprintln!(
                "DEBUG applied mv={mv:?} -> current={} civil_actions={} military_actions={} phase={:?} round={} pending_before={:?} yellow_bank={} food={}",
                self.state.current, p.civil_actions, p.military_actions, self.state.phase, self.state.round, pending_top_before, p.yellow_bank, p.food
            );
        }
        Ok(())
    }
}

/// The `Move` a player OTHER than `state.decider()` is legally allowed to
/// make right now, purely because Development of Civil Life ("Development
/// of Civilization" in BGO's UI -- `corpus.rs::ALIASES`) banked them a
/// one-time discount (`docs/REPLAY.md` Finding 2, `state::OneTimeDiscount`'s
/// own doc comment: the ONLY writer of these three fields). The real card
/// text is "Immediately, each civilization may either: increase its
/// population; or build a farm, mine or urban building; or develop a
/// technology. It costs 1 [resource] less than usual" -- an untimed grant to
/// EVERY player, not a choice scoped to whoever prepared it or to their own
/// turn, which is exactly why a real BGO player can and does spend it
/// interleaved into another player's live turn (confirmed against real BGO
/// games `7523354`, `7523355`, and every other sampled game whose replay
/// stopped on a `decider != expected actor` mismatch: all of them contain
/// this event). `None` when `class`/`card` do not match a shape Civil Life
/// can explain (including: the field is already zero, meaning either this
/// player never had the grant or already spent it on a real, in-turn action
/// -- in which case the caller's normal, `state.decider()`-gated dispatch is
/// the right path and should run instead, most likely producing an honest
/// mismatch rather than silently guessing).
///
/// `ActionClass::DevelopTechnology` (BGO logs this shape as `"<Color>
/// discovers <Card> <Color> loses N science"`, not `"develops"` -- see
/// `corpus.rs`'s `"discovers "` prefix) IS covered, unlike Pop/Build:
/// `apply::h_develop` also removes the developed card from `p.hand_civil`,
/// which is only safe to trust out of turn when this binary already
/// grounded that exact card in `actor`'s hand from an earlier, ordinary,
/// in-turn `TakeCard` line -- checked via `hand_civil.contains` below
/// rather than assumed. In every real game this fired against
/// (`docs/REPLAY.md` Finding 2), that was true: the interjecting player had
/// already taken the card into hand normally, sometimes turns earlier, and
/// was simply waiting for a chance to develop it.
///
/// Every branch below also re-checks ordinary affordability (food/
/// resources/science, and a free worker for Build) against `actor`
/// DIRECTLY, not through [`legal::legal_moves`] -- that function is scoped
/// to `state.decider()`/`state.current`, which by construction is NOT
/// `actor` here (the whole reason this helper exists). Skipping this check
/// is not merely a missed validation: this function's caller applies its
/// result via `apply::apply_free_civil_move` directly, bypassing
/// `Replayer::try_apply`'s own `legal_moves` gate entirely, so an
/// unaffordable move reaches `apply::h_pop`/`do_build`/`h_develop` with NO
/// prior legality check at all -- those functions each end in a
/// `debug_assert!` that a legal caller could never trip, which means an
/// affordability drift this binary's OWN reconstruction accumulated
/// (rather than a genuine journal-parsing gap) surfaced as a hard process
/// PANIC instead of an honest `Mismatch`, aborting the entire run and
/// losing every other game's data in the same batch. Found by replaying the
/// real BGO corpus at scale (`replaystats`): game `7522949` reached this
/// exact panic once the [`homer_unit_discount`] fix (`costs.rs`) let it run
/// deep enough to reach a second Civil Life reveal whose banked `pop_food`
/// discount this binary's own reconstructed `food`/`yellow_bank` could not
/// actually cover. Returning `None` here instead routes it back through the
/// caller's normal, `state.decider()`-gated path, which reports the SAME
/// underlying problem as a `Mismatch` -- exactly what every other
/// unaffordable-move shape in this file already does.
fn civil_life_move(r: &Replayer, actor: u8, class: ActionClass, card: Option<CardId>) -> Option<Move> {
    let p = &r.state.players[actor as usize];
    match class {
        ActionClass::IncreasePopulation if p.one_time_discount.pop_food != 0 => {
            let cost = economy::pop_cost(&r.state, p)?;
            (p.food as i32 >= cost).then_some(Move::Pop)
        }
        ActionClass::BuildBuilding if p.one_time_discount.build_resources != 0 => {
            let card = card?;
            // Civil Life only discounts a build, never grants the
            // technology itself -- `card` must already be developed (in
            // `actor`'s own tableau), and never a military unit (the
            // discount field is scoped to `URBAN_OR_PRODUCTION` cards only,
            // matching the card text's "farm, mine or urban building").
            if crate::costs::is_unit(card) || p.techs.get(card).is_none() || p.workers_free == 0 {
                return None;
            }
            let cost = costs::build_cost_for(&r.state, p, card)?;
            (p.resources as i32 >= cost).then_some(Move::Build { card })
        }
        ActionClass::DevelopTechnology if p.one_time_discount.develop_science != 0 => {
            let card = card?;
            if !p.hand_civil.contains(card) {
                return None;
            }
            let cost = costs::tech_cost(&r.state, p, card)?;
            (p.science as i32 >= cost).then_some(Move::Develop { card })
        }
        _ => None,
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

/// The number right after `" loses "` in `text`, if the SAME clause names
/// `"military resource"` -- e.g. `"Purple builds Warrior Purple loses 1
/// military resource; Purple spends 1 resource"`. BGO's UI splits a unit
/// build/upgrade's total resource payment into two clauses by SOURCE: any
/// portion covered by `p.mil_discount` (Patriotism/Wave of Nationalism/
/// Military Build-Up's "pay N fewer resources [for military units]" pool,
/// `costs::spend_mil_discount`) is printed as `"loses N military resource"`,
/// and any REMAINING portion paid from the ordinary resource pool as
/// `"spends N resource"` -- found by replaying real BGO games
/// (`docs/REPLAY.md` fifth pass): `loses` + `spends` summed always equals
/// exactly this binary's own `costs::build_cost_for` for the unit, even on
/// lines with NO preceding Patriotism-style grant visible in the journal at
/// all (a currently-unexplained baseline case -- see the doc's own notes).
/// Reading `"spends"` alone (the OLD behaviour) silently under-counted the
/// unit's total cost by exactly the `"loses"` amount, which this binary's
/// `costs::build_cost_for` never subtracts for units (`is_unit` gates BOTH
/// `one_time_discount` fields out on purpose -- Civil Life's grant text is
/// farm/mine/urban building only, never a unit) -- reported as a "build cost
/// mismatch (unmodeled discount)" that was actually a pure parsing gap, not
/// a discount this binary's `costs::build_cost_for` needed to model at all.
fn lost_military_resource(text: &str) -> Option<i32> {
    let p = text.find(" loses ")?;
    let rest = &text[p + " loses ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    if !rest[digits_end..].starts_with(" military resource") {
        return None; // e.g. "loses 1 population" -- a different clause entirely
    }
    rest[..digits_end].parse().ok()
}

/// The TOTAL a build/upgrade line actually paid: [`spent_resources`]'s
/// `"spends N resource"` clause plus [`lost_military_resource`]'s `"loses N
/// military resource"` clause, whichever of the two (or both) are present.
/// `None` only when NEITHER clause appears (a fully free build this check
/// has nothing to compare) -- see [`lost_military_resource`]'s own doc
/// comment for why summing, not choosing one, is correct.
fn total_paid_for_build(text: &str) -> Option<i32> {
    match (lost_military_resource(text), spent_resources(text)) {
        (None, None) => None,
        (lost, spent) => Some(lost.unwrap_or(0) + spent.unwrap_or(0)),
    }
}

/// What a `"<Colour> takes <Card> in hand ..."` line says the take cost.
///
/// A take line with NO `"uses N civil/military action"` clause at all cost
/// **zero** actions -- it is not an unknown. BGO prints the clause for every
/// non-zero cost, and a row take genuinely costs 0 whenever Hammurabi's
/// printed `leaderTakeCivilActionDiscount` ("when you take a new leader from
/// the card row, it costs 1 less civil action") cancels the 1 CA of a leader
/// sitting in one of the row's five cheapest slots. Measured over the whole
/// 1,011-game corpus: of 88,432 take lines, 483 carry no clause, and 333 of
/// those are LEADER takes -- **every single one with Hammurabi in play**.
/// (The other 150 are all Taj Mahal, an unexplained card-specific anomaly
/// this file deliberately does NOT paper over -- see `docs/REPLAY.md`; with
/// this function returning 0 for them they now fail as an honest
/// `ParserGap` naming the cost mismatch instead of silently landing in
/// whatever slot happened to be free.)
///
/// Treating "no clause" as `None` -- the old behaviour -- made
/// [`Replayer::ground_row_slot`] fall through to its "first ungrounded slot"
/// path, i.e. a silent guess at which slot the human actually paid for.
fn observed_take_cost(text: &str) -> i32 {
    total_action_cost(text).unwrap_or(0)
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
    let has_sides = card_id.get().special.iter().any(|s| matches!(s, crate::cards::Special::A(_)))
        && card_id.get().special.iter().any(|s| matches!(s, crate::cards::Special::B(_)));
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

/// One committed defense card, fully identified from its own clause on the
/// `"<Color> defends ..."` line -- see [`resolve_aggression_defense`]'s doc
/// for why every clause resolves to an exact `defense_bonus` requirement
/// rather than a bare count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefenseClause {
    /// `"Defense card +<bonus> played"` -- `bonus` is 2, 4 or 6 and is the
    /// card's own printed, unique-per-age identity.
    Bonus(i16),
    /// `"military card played"` -- any zero-`defense_bonus` hand card.
    Flat,
}

/// Parse every committed-card clause out of a `"<Color> defends ..."`
/// line, expanded to one [`DefenseClause`] per physically committed card
/// (every clause observed in the real corpus carries a leading count of 1,
/// but this does not assume that). `None` means `text` is not a "defends"
/// line at all. The trailing `"<Color> strength: <n>"` bookkeeping clauses
/// never match either card pattern, so they are simply skipped rather than
/// needing to be located and cut off first.
fn parse_defense_clauses(text: &str) -> Option<Vec<DefenseClause>> {
    let (_, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("defends ")?;
    let mut out = Vec::new();
    for clause in rest.split("; ") {
        let mut words = clause.split_whitespace();
        let Some(n) = words.next().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let rest_words: Vec<&str> = words.collect();
        let one = match rest_words.as_slice() {
            ["Defense", "card", bonus, "played"] => {
                bonus.strip_prefix('+').and_then(|b| b.parse::<i16>().ok()).map(DefenseClause::Bonus)
            }
            ["military", "card", "played"] => Some(DefenseClause::Flat),
            _ => None,
        };
        if let Some(one) = one {
            out.extend(std::iter::repeat(one).take(n as usize));
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------
// Pre-scan: the event preparation record
// ---------------------------------------------------------------------

/// Every `"<Color> plays event ..."` line as `(lineno, actor seat, text)` --
/// the raw input [`crate::event_plan::solve`] turns into a solved plan.
fn prescan_plays_event_lines<'t>(lines: &[Line<'t>]) -> Vec<(usize, u8, &'t str)> {
    lines
        .iter()
        .filter_map(|line| {
            let (color, rest) = actor_and_rest(line.text)?;
            rest.starts_with("plays event").then(|| (line.lineno, color.seat(), line.text))
        })
        .collect()
}

/// `"<Color> produces N food"` / `"<Color> produces N resources"` -- a
/// STANDALONE bookkeeping line (nothing else on it besides the actor and
/// this clause). `corpus.rs` classifies this shape as pure bookkeeping (not
/// a distinct action) for `corpuscensus.rs`'s purposes, which is correct for
/// counting, but `replay.rs` needs more: every sampled occurrence of this
/// EXACT shape in the corpus was found (empirically, `docs/REPLAY.md`) to be
/// a player's resolution of a `ChoiceKind::GainBlock` opened by an event
/// like "Development of Markets" ("Each civilization gains 2 resources or 2
/// food (player's choice)") -- see `resolve_intervening`'s handling of that
/// pending kind, which this feeds. Returns `None` for any line with extra
/// clauses (e.g. a colonize reveal's own trailing `"... Purple produces 3
/// food"`), which is a DIFFERENT (deterministic, non-choice) production this
/// binary must not mistake for a GainBlock resolution.
fn parse_standalone_produces(text: &str) -> Option<(Color, bool, i32)> {
    let (actor, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("produces ")?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: i32 = rest[..digits_end].parse().ok()?;
    match &rest[digits_end..] {
        " resources" => Some((actor, true, n)),
        " food" => Some((actor, false, n)),
        _ => None,
    }
}

/// The LAST `"produces N food"` / `"produces N resources"` clause anywhere
/// in `text`, no matter what precedes it -- unlike [`parse_standalone_produces`]
/// (which requires the WHOLE line to be nothing but that clause, matching a
/// `GainBlock` event's own separate bookkeeping row per player), this reads
/// a produces clause BGO glues onto the SAME row as the action that caused
/// it. Reserves (`Special::GainFoodOrResources`, `ChoiceKind::FoodOrRes`) is
/// the only base-game card with this shape: `"<Color> plays Reserves
/// <Color> produces N food"` -- one single row, no separating punctuation,
/// the actor's name repeated (confirmed against the full corpus: 4157 of
/// 4158 "plays Reserves" lines across all 1,011 games have this exact glued
/// shape; the one exception is a different anomaly, not chased here). `rfind`
/// (not `find`) in case an EARLIER, unrelated clause on the same row also
/// happens to contain " produces " (not observed for Reserves specifically,
/// but cheap insurance).
fn trailing_produces(text: &str) -> Option<(bool, i32)> {
    let p = text.rfind(" produces ")?;
    let rest = &text[p + " produces ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: i32 = rest[..digits_end].parse().ok()?;
    if rest[digits_end..].starts_with(" resources") {
        Some((true, n))
    } else if rest[digits_end..].starts_with(" food") {
        Some((false, n))
    } else {
        None
    }
}

/// The LAST `"gets N science"` clause anywhere in `text` -- Breakthrough's
/// own bonus, glued onto the SAME line as the `"using Breakthrough"` develop
/// it orders (`"<Color> discovers <Tech> using Breakthrough <Color> loses N
/// science; <Color> gets M science"`). Unlike Urban Growth/Rich Land/
/// Efficient Upgrade, Breakthrough's per-age difference (RB p.15, confirmed
/// against `sources/bga_throughtheages_material.inc.php`) is NOT a resource
/// discount at all -- it develops at full science price and then scores a
/// flat bonus (2 for the Age I copy, 3 for Age II) -- so this is the signal
/// [`resolve_named_card_by_effect`] matches against `Card::effects::
/// gain_science` for that one card, parallel to [`trailing_produces`] for
/// Frugality's `gain_food`.
fn trailing_gets_science(text: &str) -> Option<i32> {
    let p = text.rfind(" gets ")?;
    let rest = &text[p + " gets ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: i32 = rest[..digits_end].parse().ok()?;
    rest[digits_end..].starts_with(" science").then_some(n)
}

/// Resolves which age-sibling of `named` (`corpus::build_card_index`'s
/// necessarily arbitrary same-name pick -- see `best_age_sibling`'s doc
/// comment) actually produced `wanted`'s observed cost on THIS journal line,
/// by solving for the discount the payment implies and matching it against
/// `corpus::family_siblings`' printed `resourceDiscount`/`gainScience`.
/// This is strictly stronger evidence than `best_age_sibling`'s "not newer
/// than the current age" guess (an earlier `ActionClass::TakeCard` line
/// necessarily used, `age_civil` being all it had) or a bare hand search
/// (which can only find whatever that earlier guess put there) -- exact
/// evidence beats both, so it is tried FIRST; the hand search remains as a
/// fallback for the rare case a clamped-at-zero payment (`(cost -
/// discount).max(0)`) is consistent with more than one sibling's discount,
/// or the observed-cost clause is missing/unparseable altogether.
fn resolve_named_card_by_effect(state: &GameState, p: &PlayerState, named: CardId, wanted: Move, raw_text: &str) -> CardId {
    let solved = match wanted {
        Move::Build { card } => total_paid_for_build(raw_text)
            .and_then(|paid| Some(costs::build_cost_for(state, p, card)? - paid))
            .and_then(|needed| family_siblings(named).into_iter().find(|id| id.get().effects.resource_discount as i32 == needed)),
        Move::Upgrade { from, to } => total_paid_for_build(raw_text)
            .map(|paid| costs::upgrade_cost(state, p, from, to) - paid)
            .and_then(|needed| family_siblings(named).into_iter().find(|id| id.get().effects.resource_discount as i32 == needed)),
        // Breakthrough's science bonus is the SAME clause whichever half of
        // its "develop a technology OR pay for a revolution" order (RB
        // p.15, `legal::free_action_moves`'s own `DevelopTechnology` arm
        // comment) the human took -- confirmed corpus-wide: `"<Color>
        // revolutions using Breakthrough ... <Color> gets N science"` reads
        // exactly like the develop case.
        Move::Develop { .. } | Move::Revolution { .. } => trailing_gets_science(raw_text)
            .and_then(|bonus| family_siblings(named).into_iter().find(|id| id.get().effects.gain_science as i32 == bonus)),
        _ => None,
    };
    solved.unwrap_or_else(|| {
        p.hand_civil
            .as_slice()
            .iter()
            .copied()
            .find(|id| id.get().base_name == named.get().base_name)
            .unwrap_or(named)
    })
}

/// If `p`'s civil hand holds a DIFFERENT age-sibling of `correct`'s name
/// family than `correct` itself, swap it for `correct` -- a no-op when
/// `correct` is already there (the common case) or no sibling is in hand at
/// all (nothing to correct). This is `ground_row_slot`'s "grounding"
/// philosophy applied one step later: a `TakeCard` line is necessarily
/// age-blind (`best_age_sibling`'s doc comment), but a LATER `"plays"`/
/// `"using"` line's own printed numbers can pin the exact card down
/// (`resolve_named_card_by_effect`) -- when that disagrees with the earlier
/// guess, the guess was wrong, and correcting the hand entry now is more
/// honest than either silently keeping the wrong card or refusing to notice.
fn correct_hand_family(p: &mut PlayerState, correct: CardId) {
    if p.hand_civil.contains(correct) {
        return;
    }
    if let Some(wrong) = p.hand_civil.as_slice().iter().copied().find(|id| id.get().base_name == correct.get().base_name) {
        p.hand_civil.remove_first(wrong);
        p.hand_civil.push(correct);
    }
}

/// Pre-scans every standalone `"<Color> produces ..."` line in the journal
/// into a per-seat FIFO (see `parse_standalone_produces`'s doc comment for
/// why this shape means a `ChoiceKind::GainBlock` resolution). FIFO order
/// matches journal order per seat, which is safe because a `GainBlock`
/// pending for a given player blocks every other action by that SAME player
/// until resolved (confirmed empirically: every standalone `"produces"` line
/// sampled across the corpus follows a "(player's choice)" event with no
/// unrelated action by that player in between).
fn prescan_gain_produces(lines: &[Line]) -> HashMap<u8, VecDeque<(bool, i32)>> {
    let mut out: HashMap<u8, VecDeque<(bool, i32)>> = HashMap::new();
    for line in lines {
        if let Some((actor, is_resources, n)) = parse_standalone_produces(line.text) {
            out.entry(actor.seat()).or_default().push_back((is_resources, n));
        }
    }
    out
}

/// Pre-scans the whole journal once for every point where a player is
/// observed playing a NAMED card out of their own military hand --
/// `DeclareWar`, `PlayAggression`, `ProposePact`, or `PlayTactic` (excluding
/// `CopyTactic`, `"adopts existing tactics ..."`, which copies an
/// opponent's already-played tactic rather than consuming the actor's own
/// hand card -- see `apply_one`'s `ActionClass::PlayTactic` arm, which draws
/// the same distinction). Feeds `discard_solver::DiscardSolver`: a card
/// observed being played AFTER a given discard decision was, by definition,
/// still in that player's hand at the time of the discard, so it cannot
/// have been the card discarded there -- see that module's doc for the full
/// argument this pre-scan supplies the raw facts for.
fn prescan_future_military_needs(
    lines: &[Line],
    card_index: &HashMap<&'static str, CardId>,
) -> HashMap<u8, Vec<FutureNeed>> {
    let mut out: HashMap<u8, Vec<FutureNeed>> = HashMap::new();
    for line in lines {
        let LineOutcome::Action(Classified { class, card: Some(card) }) = classify(card_index, line.text) else {
            continue;
        };
        let Some((actor, rest)) = actor_and_rest(line.text) else { continue };
        let consumes_own_hand_card = match class {
            ActionClass::DeclareWar | ActionClass::PlayAggression | ActionClass::ProposePact => true,
            ActionClass::PlayTactic => !rest.starts_with("adopts existing tactics "),
            _ => false,
        };
        if consumes_own_hand_card {
            out.entry(actor.seat()).or_default().push(FutureNeed { lineno: line.lineno, card });
        }
    }
    out
}

/// Line indices to skip entirely because they are a `TakeCard` undone by a
/// same-actor, same-card `PutBack` -- BGO's client-side undo (`corpus.rs`'s
/// module doc: "~8% of raw takes are a human undoing their own take within
/// the same turn"). Rather than modelling `PutBack` as an engine `Move`
/// (there is none -- see this file's module doc), the take that never should
/// have counted is simply never applied: both journal lines are skipped as a
/// pair, which is the exact meaning of "take it back."
///
/// A take and its put-back are NOT always textually adjacent: BGO's UI lets
/// a player hold several tentative takes at once (a stack -- take A, take B,
/// put B back, put A back is a real observed pattern) and freely interleave
/// OTHER committed actions (builds, other takes, ...) in between, e.g. take
/// Frugality, take Alchemy, build a wonder stage, put Frugality back. The
/// original adjacency-only pairing (a single `last_take` slot, reset by any
/// intervening classified line) missed both shapes and reported an
/// "unpaired PutBack" mismatch that stopped replay outright -- 3/24 games in
/// the initial sample (`docs/REPLAY.md`).
///
/// Since every row/hand card is a unique instance in the table (the same
/// name never denotes two different physical cards within one game -- the
/// same assumption `ground_row_slot`/`card_index` already rely on), a
/// `PutBack` can only ever refer to a still-open take of that exact card by
/// that exact actor, so matching is done with a per-card stack of
/// not-yet-resolved takes rather than a single "last take" slot: any
/// `TakeCard` pushes onto its card's stack, any `PutBack` pops the most
/// recent entry for the same actor, and (defensively, since it should never
/// happen with unique card instances) any OTHER classified line naming that
/// same card by the same actor -- i.e. the take was committed some other way
/// (built, developed, played, elected, ...) -- removes it from the stack so
/// a later same-named "put back" (which cannot legitimately exist once a
/// card is committed) can never wrongly erase this now-real action.
fn prescan_putback_skips(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> std::collections::HashSet<usize> {
    let mut skip = std::collections::HashSet::new();
    let mut open: HashMap<CardId, Vec<(usize, Color)>> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let LineOutcome::Action(Classified { class, card }) = classify(card_index, line.text) else {
            continue; // bookkeeping: never references a card, nothing to do
        };
        let Some((actor, _)) = actor_and_rest(line.text) else { continue };
        let Some(c) = card else { continue };
        match class {
            ActionClass::TakeCard => open.entry(c).or_default().push((i, actor)),
            ActionClass::PutBack => {
                if let Some(stack) = open.get_mut(&c) {
                    if let Some(pos) = stack.iter().rposition(|&(_, a)| a == actor) {
                        let (take_i, _) = stack.remove(pos);
                        skip.insert(take_i);
                        skip.insert(i);
                    }
                }
            }
            _ => {
                if let Some(stack) = open.get_mut(&c) {
                    stack.retain(|&(_, a)| a != actor);
                }
            }
        }
    }
    skip
}

// ---------------------------------------------------------------------
// Per-game replay
// ---------------------------------------------------------------------

pub struct GameResult {
    pub id: String,
    pub players: u8,
    pub actions_consumed: usize,
    pub completed: bool,
    pub mismatch: Option<Mismatch>,
    pub colonize_approximated: bool,
    pub engine_scores: Option<Vec<i32>>,
    pub index_scores: Vec<i32>,
    /// Counts from this game's `DiscardSolver` -- see that module's doc and
    /// `docs/REPLAY.md` for why these three are reported separately rather
    /// than folded into one "discards handled" number.
    pub discards_solved: u32,
    pub discards_chosen: u32,
    pub discards_forced_collision: u32,
    /// Every human decision point recorded along the way -- empty unless
    /// `replay_game` was called with `record_decisions: true`. See the
    /// module doc's "Decision recording" section.
    pub decisions: Vec<Decision>,
}

/// A journal line class that is BGO's after-the-fact CONFIRMATION that
/// something already resolved, carrying no state of its own to apply --
/// `apply_one`'s handling of all three is a bare `Ok(())`. `resolve_
/// intervening` must NOT be called for one of these: its job is to clear a
/// path to whatever `expected_actor` needs to do NEXT, and calling it here
/// makes it try to fast-forward PAST a decision using a fallback meant for
/// cases where the journal never logs that decision at all (`FreeBuild`'s
/// "assume Skip", the `Pending::Auction` auto-`BidPass`), silently consuming
/// a real human decision instead of leaving it for the line that actually
/// supplies it. Two different mechanisms produce the same "confirmation
/// line reached with the wrong decider" shape:
///
/// - `PlayEvent`/`WinAuction`: BGO logs the confirmation BEFORE the real
///   action that causes it (the qualifying players' own `FreeBuild`/
///   `GainBlock` picks; the last active bidder's own `"passes"`/`"bids"`
///   line).
/// - `Colonize`: the auction's own last `Bid`/`BidPass` line already drove
///   the winner's ENTIRE colonize sequence to completion synchronously
///   (`interact::auction_move` -> `colonize`, both auto-resolving whenever
///   only one legal continuation exists) as a side effect of resolving a
///   DIFFERENT player's (the auction revealer's) own political decision --
///   by the time the `"<Color> colonizes ..."` confirmation line is
///   reached, `state.current` has already returned to whoever's turn
///   triggered the auction in the first place, not the colonizer, so
///   `decider() != expected_actor` even though nothing is actually wrong.
///
/// See the three call sites' own doc comments (above the single call to
/// this function in `replay_game`'s main loop) for the specific real games
/// each was found on.
fn is_pure_confirmation_line(class: ActionClass) -> bool {
    matches!(class, ActionClass::PlayEvent | ActionClass::WinAuction | ActionClass::Colonize)
}

/// Replay one game's journal through the real engine. `record_decisions`
/// gates [`Decision`] snapshotting (see the module doc) -- `false` for
/// `replay`'s own binary, `true` for `agreement.rs`'s move-agreement
/// analysis.
pub fn replay_game(
    meta: &GameMeta,
    journal_text: &str,
    card_index: &HashMap<&'static str, CardId>,
    record_decisions: bool,
) -> GameResult {
    let lines = parse_lines(journal_text);
    let putback_skips = prescan_putback_skips(&lines, card_index);
    let gain_produces = prescan_gain_produces(&lines);
    let future_military_needs = prescan_future_military_needs(&lines, card_index);

    let mut mismatch: Option<Mismatch> = None;
    let mut completed = false;

    // Solved before the first line is read, because it is a whole-game
    // constraint problem, not a per-decision one -- see `event_plan`'s
    // module doc. An infeasible journal stops the game at the line that
    // contradicts the pile model rather than being softened into a guess.
    let plan = match crate::event_plan::solve(&prescan_plays_event_lines(&lines), card_index, meta.players) {
        Ok(plan) => plan,
        Err(err) => {
            let lineno = match err {
                crate::event_plan::EventPlanError::NoCultureClause { lineno }
                | crate::event_plan::EventPlanError::UnknownRevealedCard { lineno, .. }
                | crate::event_plan::EventPlanError::BatchAgeMismatch { lineno, .. } => lineno,
            };
            let at = lines.iter().find(|l| l.lineno == lineno);
            mismatch = at.map(|l| mk_mismatch(l, MismatchKind::EventPlanInfeasible(err.to_string())));
            EventPlan::default()
        }
    };
    // Nothing is replayed at all when the plan is infeasible: the event
    // record is a whole-game fact, so a contradiction in it invalidates
    // every turn, not just the one that exposed it.
    let journal: &[Line] = if mismatch.is_some() { &[] } else { &lines };
    let mut r = Replayer::new(card_index, meta.players, plan, gain_produces, future_military_needs);
    r.record_decisions = record_decisions;

    'lines: for (i, line) in journal.iter().enumerate() {
        if line.text.starts_with("End of game") {
            completed = true;
            break;
        }
        if putback_skips.contains(&i) {
            continue;
        }
        r.current_lineno = line.lineno;
        let outcome = classify(card_index, line.text);
        let LineOutcome::Action(Classified { class, card }) = outcome else {
            continue; // bookkeeping / unclassified: no move to apply
        };
        let Some((actor_color, rest)) = actor_and_rest(line.text) else {
            // EndTurn lines start with "End turn", no leading colour --
            // the actor is whoever the engine currently has as `current`.
            if class == ActionClass::EndTurn {
                let actor = r.state.current;
                r.auto_passed[actor as usize] = 0;
                if let Err(kind) = r
                    .resolve_intervening(actor, (class, None), false)
                    .and_then(|()| r.try_apply(Move::EndTurn, true))
                {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
                }
                r.actions_consumed += 1;
                continue;
            }
            // Alexander the Great's death line, also no leading colour --
            // the actor is named only in the trailing "<Color> gets 1
            // yellow token" clause (`corpus::classify`'s own comment). This
            // IS the player's own political decision (the alternative to
            // whatever else they might have done with their political
            // action), so `next_line_explains_own_politics: true`, the same
            // as `ChangeGovernment`/`Pass`/etc: `resolve_intervening` must
            // stop and let THIS line apply rather than auto-resolving a
            // different political move first.
            if class == ActionClass::RemoveLeaderYellow {
                let Some(actor_color) = color_after(line.text, "Empire ") else {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("Alexander death line missing its trailing actor colour".into())));
                    break 'lines;
                };
                let actor = actor_color.seat();
                if actor >= meta.players {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("actor colour {actor_color:?} outside {}p seating", meta.players))));
                    break 'lines;
                }
                if let Err(kind) = r
                    .resolve_intervening(actor, (class, None), true)
                    .and_then(|()| r.try_apply(Move::RemoveLeaderYellow, true))
                {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
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

        // Development of Civil Life ("Development of Civilization" in BGO's
        // UI) grants EVERY player a banked, untimed `one_time_discount` the
        // instant it resolves -- spendable whenever that player likes,
        // including mid-ANOTHER-player's-turn, since the grant is not
        // gated on whose turn it is (`civil_life_move`'s own doc comment;
        // `docs/REPLAY.md` Finding 2). Checked and applied here, BEFORE
        // `resolve_intervening`/`apply_one`, because both of those assume
        // the acting player IS (or is about to become) `state.decider()` --
        // true for every other kind of action this binary handles, never
        // true for this one. `actor != r.state.decider()` is the gate: a
        // player exercising their OWN discount on their OWN turn (the
        // common case) still goes through the normal path below.
        if actor != r.state.decider() && r.state.pending.is_empty() {
            if let Some(mv) = civil_life_move(&r, actor, class, card) {
                apply::apply_free_civil_move(&mut r.state, actor, mv, 0);
                r.actions_consumed += 1;
                continue 'lines;
            }
        }

        let explains_own_politics = matches!(
            class,
            ActionClass::Pass
                | ActionClass::ChangeGovernment
                | ActionClass::DeclareWar
                | ActionClass::PlayAggression
                | ActionClass::ProposePact
        );
        // `ActionClass::PlayEvent` (`"X plays event ..."`) is the
        // JOURNAL-side confirmation that an event already resolved -- see
        // the module doc's "Event/Territory preparation" section: the
        // engine resolves an event's ENTIRE effect (gains, `queue_decisions`
        // for every qualifying player -- e.g. a `FreeBuild`/`GainBlock`
        // choice opened for each of them at once) synchronously, inside
        // `h_prepare_event`, the instant the hidden `PrepareEvent` is
        // inferred; `apply_one`'s own `PlayEvent` arm is a bare `Ok(())`
        // that reads no state at all. Calling `resolve_intervening` for
        // THIS line was actively harmful: its job is to make `legal_moves`
        // offer whatever comes NEXT, so when that "next" is the PlayEvent
        // no-op itself, its one-line lookahead (`upcoming = (PlayEvent,
        // None)`) can never match a `FreeBuild`/`GainBlock` pending's own
        // options -- and the FreeBuild branch's fallback IS "assume Skip",
        // so it silently, wrongly discarded every qualifying player's real
        // free-build choice before the following lines (which DO ask for
        // it) were ever read. Skipping the call here defers resolution
        // (including the hidden-`PrepareEvent` inference itself, if it
        // hasn't fired yet) to whatever the NEXT real line's own
        // `resolve_intervening` call needs -- which has the right
        // `upcoming` to match against -- found by testing against a real 3p
        // game where "Development of Religion" (a `FreeBuild` event) opened
        // a choice for all 3 players and every one of them was wrongly
        // auto-skipped before their own real "builds Religion" line was
        // even read (`docs/REPLAY.md`).
        //
        // `ActionClass::Discard` (`"<Color> discards N card(s)"`) still
        // NEEDS this call, unlike `PlayEvent` -- `resolve_intervening`'s own
        // `ChoiceKind::DiscardMilitary` branch has a `matches_upcoming` case
        // (mirroring `FreeBuild`'s) that returns `Ok(())` without draining
        // when the open pending IS `expected_actor`'s own and the upcoming
        // line IS their `"discards"` line, deferring the actual `Move::
        // Choose` to `apply_one`'s `Discard` arm (`resolve_discard`) --
        // see that branch's doc for why resolving it any earlier would
        // wrongly report a stuck pending (resolving the LAST queued discard
        // can itself finish `actor`'s end of turn and advance `state.
        // current`, making the generic `decider == expected_actor` exit
        // test fail even though nothing went wrong).
        //
        // `ActionClass::WinAuction` (`"X wins <Territory> Winning bid is
        // N"`) is the SAME shape as `PlayEvent` above, for the SAME reason:
        // it is BGO's journal-side CONFIRMATION that a colonize auction
        // already concluded, not an action with its own state to apply --
        // `apply_one`'s `WinAuction` arm is a bare `Ok(())`. The auction
        // itself is driven entirely by the real `Bid`/`BidPass` ("bids
        // N"/"passes") lines, and BGO's own journal orders the "wins"
        // confirmation BEFORE the final bidder's own explicit pass that
        // actually causes it (observed on real games with identical
        // one-second timestamps on both lines -- the same "not stably
        // ordered within a second" artifact `docs/REPLAY.md`'s Taj Mahal
        // section already documents for a different pair of lines).
        // Calling `resolve_intervening` for THIS line was actively harmful
        // for the identical reason `PlayEvent` was excluded: its job is to
        // clear a path to whatever `expected_actor` (here, the auction's
        // WINNER, parsed from the "wins" line's own text) needs next -- and
        // since the true decider is still the LAST ACTIVE BIDDER, not the
        // winner, it fell through to the `Pending::Auction` auto-drain
        // fallback and synthesized a FAKE `Move::BidPass` for that bidder on
        // the spot, consuming the pending before their own real, upcoming
        // "passes"/"bids" line was ever read -- which then had nothing left
        // to apply and reported `decider != expected_actor` instead. Found
        // by replaying real BGO games (`7522652`, `7523072`): 59 of this
        // bucket's 148 stops shared this exact shape. Skipping the call
        // here, exactly like `PlayEvent`, defers the auction's real
        // resolution to the next real `Bid`/`BidPass` line, which has the
        // correct `upcoming` to match against; the pre-existing
        // `Pending::Auction` auto-drain fallback is untouched and still
        // covers a game where the final bidder's own pass is genuinely never
        // logged at all.
        if !is_pure_confirmation_line(class) {
            if let Err(kind) = r.resolve_intervening(actor, (class, card), explains_own_politics) {
                mismatch = Some(mk_mismatch(line, kind));
                break 'lines;
            }
        }

        let next_text = journal.get(i + 1).map(|l| l.text);
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
        discards_solved: r.discard_solver.solved,
        discards_chosen: r.discard_solver.chosen,
        discards_forced_collision: r.discard_solver.forced_collisions,
        decisions: r.decisions,
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

/// If `rest` (an action's own journal text, e.g. `"builds ..."`/
/// `"discovers ..."`/`"upgrades ..."`, text after the actor's colour) names
/// a `"using <Card>"` discount source that is a `FreeCivilAction`-granting
/// Action card currently in `actor`'s hand, plays it (`Move::PlayAction`)
/// and resolves the ordered action onto `wanted`, returning `Ok(true)` once
/// fully applied. Returns `Ok(false)` when there is no such discount source
/// named at all (the caller falls back to a plain, full-price `Move`)
/// rather than when there IS one but something about it fails to resolve
/// (that's an `Err`, not a silent fallback -- see the module doc's "gives up
/// on" list for why this file never guesses).
///
/// Shared by every ordered-action shape that BGO phrases as `"<target
/// action> ... using <Card>"` -- i.e. every `FreeCivilActionValue` that
/// needs a specific CARD named to disambiguate which of possibly several
/// tableau cards the order applies to: `Move::Build` (Rich Land/Urban
/// Growth), `Move::Upgrade` (Rich Land/Urban Growth/Efficient Upgrade --
/// found corpus-wide as `"upgrades X to Y using ..."`, previously
/// unhandled), `Move::Develop` (Breakthrough). The other two
/// `FreeCivilActionValue`s (`IncreasePopulation`, `BuildOneWonderStage`)
/// have no card to disambiguate -- BGO phrases those as `"plays <Card>
/// <effect>"` instead, glued onto the SAME line as the `PlayAction`, which
/// `ActionClass::PlayActionCard`'s own handler resolves directly rather
/// than through this "using" search (see its module doc comment there).
///
/// `landed_in_techs`: the `CardId` that should be sitting in `actor`'s
/// `techs` tableau once `wanted` has actually happened (the built/
/// upgraded-to/developed card) -- used ONLY to disambiguate the "no pending
/// opened" case below, never to decide `wanted` itself.
fn free_civil_action_move(
    r: &mut Replayer,
    actor: u8,
    rest: &str,
    wanted: Move,
    landed_in_techs: CardId,
) -> Result<bool, MismatchKind> {
    let Some(using_pos) = rest.find(" using ") else {
        return Ok(false);
    };
    let after_using = &rest[using_pos + " using ".len()..];
    let Some((named_card, _)) = longest_known_card_prefix(r.card_index, after_using) else {
        return Ok(false);
    };
    // `named_card` is `corpus::build_card_index`'s arbitrary pick among
    // same-named cards (the journal's "using <Card>" text never carries an
    // age tag, and four of these six cards recur once per age with a bigger
    // `resourceDiscount`/`gainScience` each time). Re-resolve to whichever
    // age this LINE's own observed cost actually implies -- see
    // `resolve_named_card_by_effect`'s doc comment for why that beats both
    // the arbitrary pick and a bare hand search.
    let discount_card =
        resolve_named_card_by_effect(&r.state, &r.state.players[actor as usize], named_card, wanted, rest);
    let grants_free_civil = discount_card
        .get()
        .special
        .iter()
        .any(|s| matches!(s, crate::cards::Special::FreeCivilAction(_)));
    if !grants_free_civil {
        return Ok(false);
    }
    // The evidence above can name a sibling different from whatever an
    // earlier `ActionClass::TakeCard` line (necessarily age-blind at the
    // time, see `best_age_sibling`'s doc) put in hand -- correct that now,
    // the same "ground it the instant a slot/card is taken OR PLAYED"
    // philosophy the module doc states for every other observed fact.
    correct_hand_family(&mut r.state.players[actor as usize], discount_card);
    if !r.state.players[actor as usize].hand_civil.contains(discount_card) {
        return Ok(false);
    }
    r.try_apply(Move::PlayAction { card: discount_card }, true)?;
    match r.state.pending.top() {
        Some(Pending::Choice(c)) if matches!(c.kind, ChoiceKind::FreeCivil { .. }) => {
            let n = c
                .options
                .as_slice()
                .iter()
                .position(|o| matches!(o, ChoiceOption::Move(m) if *m == wanted))
                .ok_or_else(|| {
                    MismatchKind::ParserGap(format!(
                        "{}'s free-civil-action options {:?} do not include {wanted:?}",
                        discount_card.get().name,
                        c.options.as_slice(),
                    ))
                })?;
            r.try_apply(Move::Choose { n: n as u8 }, true)?;
            Ok(true)
        }
        // `interact::push_choice`'s own `auto` behaviour never opens a
        // pending at all when there is exactly ONE candidate -- it applies
        // that candidate immediately instead (`push_choice`'s doc comment).
        // Since the journal already tells us the human's real target
        // (`wanted`), the common case here IS that immediate auto-resolve,
        // not a genuine gap -- confirmed, not assumed, by checking the
        // target card actually landed where `wanted` would have put it,
        // so an engine auto-pick that silently diverged from the journal
        // (a real bug) still surfaces as an error rather than being
        // swallowed here.
        // `landed_in_techs` covers every ordinary Build/Upgrade/Develop
        // target, but a Government card develops into `p.government`
        // instead of `techs` (`apply::h_develop`'s own dispatch) -- the one
        // shape `DevelopTechnology`'s call site can hit (Breakthrough may
        // also pay for a revolution, `legal::free_action_moves`'s own
        // comment), so it is checked here too rather than special-cased per
        // caller.
        _ if r.state.players[actor as usize].techs.has(landed_in_techs)
            || r.state.players[actor as usize].government == landed_in_techs =>
        {
            Ok(true)
        }
        _ => Err(MismatchKind::StuckPending(format!(
            "played {} for its free-civil-action discount, but no Choice pending opened and \
             {wanted:?} was not auto-resolved either",
            discount_card.get().name
        ))),
    }
}

/// Distinguishes a genuine engine/parser disagreement on a rejected `"<Color>
/// bids N"` line from this file's own documented "`hand_military` is
/// SIMULATED filler for essentially its entire content" gap (this module's
/// top doc comment, and `docs/REPLAY.md`'s "military-hand cards are never
/// named at draw time" finding): a military bonus card enters a real
/// player's hand via an anonymous end-of-turn draw and is never grounded to
/// its true identity unless the journal later shows it PLAYED (`interact::
/// bonus_pool` -- read by [`interact::max_force`], the auction ceiling --
/// reads `p.hand_military` directly, so an unplayed real bonus card this
/// binary never observed is invisible to it). §11.3's colonization force is
/// therefore only a LOWER bound here, not an exact figure, for any bidder
/// who might be holding one.
///
/// Returns `Some(UnrecoverableHiddenInfo)` only when the rejection is
/// SPECIFICALLY explained by that gap: `n` is a genuine raise (exceeds the
/// auction's current high bid, so it is not some other kind of malformed
/// bid) against the correct, currently-deciding bidder (`actor == a.
/// player`, so this is not an acting-player mismatch), and it exceeds this
/// binary's own computed ceiling. Returns `None` (keep the original
/// `IllegalMove`) for every other shape, including the auction having
/// already closed by the time this line is reached -- that is a different,
/// already-documented gap (the colonize-sacrifice auto-drain approximation
/// consuming more of an earlier winner's own units than the journal's own
/// `"Sacrificed Units:"` line says it spent, see [`Replayer::
/// auto_drain_colonize`]), not this one, and deserves its own honest
/// `IllegalMove` / `StuckPending` report rather than being folded in here.
fn bid_ceiling_mismatch(r: &Replayer, actor: u8, n: u8) -> Option<MismatchKind> {
    let Some(Pending::Auction(a)) = r.state.pending.top() else { return None };
    if a.player != actor {
        return None;
    }
    if n as i32 <= a.bid as i32 {
        return None;
    }
    let ceiling = crate::interact::max_force(&r.state, &r.state.players[a.player as usize]);
    if n as i32 <= ceiling {
        return None;
    }
    Some(MismatchKind::UnrecoverableHiddenInfo(format!(
        "colonization bid of {n} exceeds this binary's computed force ceiling ({ceiling}) for \
         the correctly-resolved bidder -- a military bonus card sitting unplayed in their hand \
         is SIMULATED filler, not a reconstructed identity, until the journal shows it played \
         (not a parser gap: the bidder and the auction are both correctly resolved)"
    )))
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
            // `card` may be `corpus::build_card_index`'s arbitrary same-name
            // pick (`best_age_sibling`'s own doc comment) -- re-resolve to
            // the highest age not newer than the civil deck's current age,
            // the best available reading of "the copy actually in the row"
            // for the nine card families BGO's journal text never tags with
            // an age. A no-op for every other card (only one age exists).
            let card = best_age_sibling(card, r.state.age_civil);
            let cost = observed_take_cost(raw_text);
            let slot = r.ground_row_slot(actor, card, Some(cost)).ok_or_else(|| {
                MismatchKind::ParserGap(format!(
                    "{}'s take cost per the journal is {cost} civil action(s), but no available \
                     row slot reproduces that under this binary's own cost formula",
                    card.get().name
                ))
            })?;
            r.try_apply(Move::Take { slot }, true)?;
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
                    return r.try_apply(Move::Choose { n: n as u8 }, true);
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
            if free_civil_action_move(r, actor, rest, Move::Build { card }, card)? {
                return Ok(());
            }
            if let (Some(want), Some(got)) = (
                total_paid_for_build(raw_text),
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
            r.try_apply(Move::Build { card }, true)
        }
        ActionClass::BuildWonderStage => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("wonder stage with no resolved card".into()))?;
            let after_builds = rest.strip_prefix("builds ").ok_or_else(|| MismatchKind::ParserGap("wonder-stage line missing 'builds '".into()))?;
            let steps = wonder_stage_count(after_builds).ok_or_else(|| MismatchKind::ParserGap("could not parse wonder stage count".into()))?;
            let _ = card; // the wonder itself is implicit in state (under construction)
            r.try_apply(Move::WonderStep { steps }, true)
        }
        ActionClass::IncreasePopulation => {
            let legal = legal::legal_moves(&r.state);
            if legal.as_slice().contains(&Move::Pop) {
                r.try_apply(Move::Pop, true)
            } else if legal.as_slice().contains(&Move::PopFree) {
                r.try_apply(Move::PopFree, true)
            } else {
                // Neither is legal -- almost always food/yellow-bank drift
                // from an earlier build/economy step this binary priced
                // differently than the true game (see the module doc's
                // "gives up on" list and `docs/REPLAY.md`'s mismatch
                // categories), not a parser gap in THIS line.
                if std::env::var("REPLAY_DEBUG").is_ok() {
                    let p = &r.state.players[actor as usize];
                    eprintln!(
                        "DEBUG Pop fail: food={} yellow_bank={} civil_actions={} pop_cost={:?} round={} numplayers={} lineno={} otd_pop_food={} raw={:?}",
                        p.food, p.yellow_bank, p.civil_actions,
                        crate::economy::pop_cost(&r.state, p), r.state.round, r.state.num_players,
                        r.current_lineno, p.one_time_discount.pop_food, raw_text
                    );
                }
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
            // `"upgrades X to Y using <Card>"` -- Rich Land/Urban Growth
            // (`BuildOrUpgradeFarmOrMine`/`BuildOrUpgradeUrbanBuilding`) and
            // Efficient Upgrade (`UpgradeFarmMineOrUrbanBuilding`) all order
            // a farm/mine/urban-building upgrade the SAME "using" way an
            // ordered Build does -- found corpus-wide (hundreds of
            // occurrences of e.g. `"upgrades Bronze to Iron using Rich
            // Land"`/`"... using Efficient Upgrade"`), previously handled
            // only for `Move::Build`. `UpgradeUnit` targets never carry a
            // "using" clause (no `FreeCivilActionValue` covers unit
            // upgrades), so this is always `Ok(false)` for that arm and the
            // bare `Move::Upgrade` below fires as before.
            if free_civil_action_move(r, actor, rest, Move::Upgrade { from, to }, to)? {
                return Ok(());
            }
            r.try_apply(Move::Upgrade { from, to }, true)
        }
        ActionClass::DevelopTechnology => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("develop with no resolved card".into()))?;
            // `"discovers X using Breakthrough"` -- the same ordered-action
            // shape as an ordered Build/Upgrade, previously unhandled here
            // (found by replaying a real 4p game: Breakthrough left in
            // `hand_civil` after its own "using" line, silently inflating
            // the reconstructed civil-hand size past the true one and
            // blocking a LATER, unrelated `Take` on a phantom hand-limit
            // wall).
            if free_civil_action_move(r, actor, rest, Move::Develop { card }, card)? {
                return Ok(());
            }
            r.try_apply(Move::Develop { card }, true)
        }
        ActionClass::ElectLeader => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("elect with no resolved card".into()))?;
            r.try_apply(Move::PlayLeader { card }, true)
        }
        ActionClass::ChangeGovernment => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("revolution with no resolved card".into()))?;
            // RB p.15: Breakthrough may spend its order on a revolution
            // instead of a develop (`legal::free_action_moves`'s own
            // `DevelopTechnology` arm) -- BGO phrases that the same "using
            // <Card>" way as an ordered Build/Upgrade/Develop
            // (`"<Color> revolutions using Breakthrough Change government
            // to ..."`), previously unhandled here entirely (this arm never
            // even looked for a "using" clause, so every such line failed
            // as a bare, illegally-free `Move::Revolution`).
            if free_civil_action_move(r, actor, rest, Move::Revolution { card }, card)? {
                return Ok(());
            }
            r.try_apply(Move::Revolution { card }, true)
        }
        ActionClass::PlayActionCard => {
            let named = card.ok_or_else(|| MismatchKind::ParserGap("play-action with no resolved card".into()))?;
            // Same age ambiguity `free_civil_action_move`'s "using <Card>"
            // sites resolve via `resolve_named_card_by_effect`: Frugality
            // and Engineering Genius are the two `FreeCivilActionValue`s
            // with no card to disambiguate in a "using" clause (that
            // function's own doc comment), so BGO glues their WHOLE order
            // onto THIS "plays <Card> <effect>" line instead -- but each
            // still states its own age-dependent number right there:
            // Frugality's post-Pop bonus (`"produces N food"`, matched
            // against `gain_food`) or Engineering Genius's wonder-stage
            // discount (implied by this line's own `"spends N resources"`
            // against the stage's undiscounted cost). Gated on
            // `FreeCivilAction` + the specific kind so this never touches
            // an unrelated action card's (Patriotism's, Reserves', ...)
            // OWN same-shaped clauses.
            let kind = named
                .get()
                .special
                .iter()
                .find_map(|s| match s {
                    crate::cards::Special::FreeCivilAction(v) => Some(legal::free_action_kind_of(*v)),
                    _ => None,
                });
            let p = &r.state.players[actor as usize];
            let solved = match kind {
                Some(legal::FreeActionKind::IncreasePopulation) => trailing_produces(raw_text)
                    .filter(|(is_resources, _)| !is_resources)
                    .and_then(|(_, n)| family_siblings(named).into_iter().find(|id| id.get().effects.gain_food as i32 == n)),
                Some(legal::FreeActionKind::BuildOneWonderStage) if !p.wonder.is_none() => {
                    let stage_cost = costs::wonder_stage_cost(&r.state, p, 1);
                    total_paid_for_build(raw_text)
                        .map(|paid| stage_cost - paid)
                        .and_then(|needed| family_siblings(named).into_iter().find(|id| id.get().effects.resource_discount as i32 == needed))
                }
                _ => None,
            };
            let card = solved.unwrap_or_else(|| {
                r.state.players[actor as usize]
                    .hand_civil
                    .as_slice()
                    .iter()
                    .copied()
                    .find(|id| id.get().base_name == named.get().base_name)
                    .unwrap_or(named)
            });
            correct_hand_family(&mut r.state.players[actor as usize], card);
            r.try_apply(Move::PlayAction { card }, true)?;
            // Reserves (`Special::GainFoodOrResources`) opens a
            // `ChoiceKind::FoodOrRes` the instant it's played, with no
            // ordered action ahead of it (`apply.rs`'s own doc comment: "a
            // real choice") -- resolve it here, from the SAME line's own
            // trailing `"produces N food/resources"` clause, rather than
            // leaving it open for a LATER line to trip over (found by
            // testing against a real 2p game, `7523818`: an unresolved
            // `FoodOrRes` blocked a `WonderStep` several lines later --
            // `trailing_produces`'s own doc comment has the corpus-wide
            // shape check justifying this fix).
            if let Some(Pending::Choice(c)) = r.state.pending.top().cloned() {
                if matches!(c.kind, ChoiceKind::FoodOrRes { .. }) {
                    let (is_resources, _n) = trailing_produces(raw_text).ok_or_else(|| {
                        MismatchKind::ParserGap(format!(
                            "PlayAction {{{}}} opened a FoodOrRes choice but no trailing \"produces\" \
                             clause found in {raw_text:?}",
                            card.get().name
                        ))
                    })?;
                    let want = if is_resources { Keyword::Resources } else { Keyword::Food };
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Word(k) if *k == want))
                        .ok_or_else(|| {
                            MismatchKind::ParserGap(format!(
                                "FoodOrRes options {:?} do not offer the journal-observed {}",
                                c.options.as_slice(),
                                if is_resources { "resources" } else { "food" }
                            ))
                        })?;
                    r.try_apply(Move::Choose { n: n as u8 }, true)?;
                } else if matches!(c.kind, ChoiceKind::FreeCivil { .. }) {
                    // Frugality (`IncreasePopulation`) and Engineering
                    // Genius's own wonder-stage order (`BuildOneWonderStage`)
                    // are the two `FreeCivilActionValue`s with no card to
                    // disambiguate, so BGO glues the WHOLE order onto this
                    // "plays <Card> <effect>" line instead of phrasing it as
                    // "<effect> using <Card>" the way Build/Upgrade/Develop's
                    // orders are (see `free_civil_action_move`'s doc). Both
                    // `legal::free_action_moves` branches for these two kinds
                    // return at most ONE candidate move (`Move::Pop` has no
                    // parameters; a player can only ever have one wonder in
                    // progress) -- picking whichever of the two is present is
                    // therefore not a guess between alternatives, only a
                    // dispatch on which kind this card is.
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Move(Move::Pop) | ChoiceOption::Move(Move::WonderStep { .. })))
                        .ok_or_else(|| {
                            MismatchKind::ParserGap(format!(
                                "played {} for its free-civil-action discount but its options {:?} \
                                 are neither Pop nor WonderStep",
                                card.get().name,
                                c.options.as_slice()
                            ))
                        })?;
                    r.try_apply(Move::Choose { n: n as u8 }, true)?;
                }
            }
            Ok(())
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
                    return r.try_apply(Move::Choose { n: n as u8 }, true);
                }
            }
            r.try_apply(Move::Destroy { card }, true)
        }
        ActionClass::PlayTactic => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("tactic with no resolved card".into()))?;
            if rest.starts_with("adopts existing tactics ") {
                r.try_apply(Move::CopyTactic { card }, true)
            } else {
                r.ground_military_hand(actor, card);
                r.try_apply(Move::PlayTactic { card }, true)
            }
        }
        ActionClass::DeclareWar => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("war with no resolved card".into()))?;
            let target = color_after(rest, " on ").ok_or_else(|| MismatchKind::ParserGap("could not parse war target colour".into()))?;
            r.ground_military_hand(actor, card);
            r.try_apply(Move::War { card, target: target.seat() }, true)
        }
        ActionClass::WinWar => Ok(()), // automatic (game::resolve_war_outcome); validation checkpoint only
        ActionClass::PlayAggression => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("aggression with no resolved card".into()))?;
            let target = color_after(rest, " against ").ok_or_else(|| MismatchKind::ParserGap("could not parse aggression target colour".into()))?;
            r.ground_military_hand(actor, card);
            r.try_apply(Move::Aggression { card, target: target.seat() }, true)?;
            resolve_aggression_defense(r, next_text)
        }
        ActionClass::ProposePact => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("pact with no resolved card".into()))?;
            let target = color_after(rest, " to ").ok_or_else(|| MismatchKind::ParserGap("could not parse pact target colour".into()))?;
            let side = pact_side(raw_text, target_actor_color(actor), card);
            r.ground_military_hand(actor, card);
            r.try_apply(Move::OfferPact { card, target: target.seat(), side }, true)
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
            r.try_apply(Move::Choose { n: n as u8 }, true)
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
            match r.try_apply(Move::Bid { n }, true) {
                Err(MismatchKind::IllegalMove { attempted, legal_moves }) => {
                    Err(bid_ceiling_mismatch(r, actor, n).unwrap_or(MismatchKind::IllegalMove { attempted, legal_moves }))
                }
                other => other,
            }
        }
        ActionClass::WinAuction => Ok(()), // automatic settlement of Pending::Auction; validation checkpoint only
        ActionClass::Pass => {
            if matches!(r.state.pending.top(), Some(Pending::Auction(_))) {
                return r.try_apply(Move::BidPass, true);
            }
            // `actor`'s politics phase is already closed and this file is
            // the one that closed it, on their behalf, earlier in this same
            // turn -- so this line is BGO's confirmation of that very pass,
            // logged late (see `auto_passed`'s doc). A pass has no effect
            // beyond closing the phase, so where it lands among the
            // player's own Action-phase lines cannot change anything. Any
            // OTHER "passes" line with nothing to apply is still a real
            // mismatch and still stops the game.
            let already_closed = r.state.phase != Phase::Politics || r.state.decider() != actor;
            if already_closed && r.auto_passed[actor as usize] > 0 {
                r.auto_passed[actor as usize] -= 1;
                return Ok(());
            }
            r.try_apply(Move::PolPass, true)
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
        // The card(s) BGO logs only a count for ("<Color> discards N
        // cards") -- resolved HERE, not left as a bare validation
        // checkpoint like `PlayEvent`/`WinWar`/`WinAuction`/`Colonize`
        // above, because resolving the LAST queued discard can itself
        // finish `actor`'s end of turn and advance `state.current`
        // (`resolve_intervening`'s `DiscardMilitary` branch's doc explains
        // why that rules out resolving it any earlier). `resolve_discard`
        // is a no-op if nothing is pending (the discard was already fully
        // drained by `resolve_intervening`'s own handling of a DIFFERENT
        // player's stale pending reached along the way here -- the
        // "different code path" shape `docs/REPLAY.md` used to report as a
        // separate, unrecoverable stop). See `discard_solver`'s module doc
        // for how the card is picked.
        ActionClass::Discard => {
            r.resolve_discard(actor);
            Ok(())
        }
        ActionClass::EndTurn => r.try_apply(Move::EndTurn, true),
        // Unreachable: the dispatch loop in `replay_game` special-cases
        // `RemoveLeaderYellow` before it ever reaches this function, the
        // same way it special-cases `EndTurn` -- both are the only two
        // `ActionClass`es whose journal line carries no leading actor
        // colour, so both need the actor resolved before `apply_one`'s
        // normal `actor` parameter (already committed to by then) would
        // even be correct.
        ActionClass::RemoveLeaderYellow => {
            Err(MismatchKind::ParserGap("RemoveLeaderYellow should have been resolved before apply_one".into()))
        }
    }
}

/// After applying an `Aggression`, resolve the victim's `Pending::Defense`
/// using the very next `"<Color> defends ..."` bookkeeping line, if any. If
/// no `Pending::Defense` opened at all (the victim had nothing eligible to
/// spend), this is a no-op. If the next line isn't a "defends" line at all,
/// that means BGO didn't log one for a defense that DID open -- treated as
/// 0 committed (the common case: RB, committing defense cards is rare and
/// costly).
///
/// Each [`DefenseClause`] `parse_defense_clauses` finds is applied as one
/// `Move::Defend`, in order:
/// - `Bonus(b)`: [`defense_bonus_card`] finds the unique card that prints
///   `defense_bonus == b` and grounds it into the defender's hand
///   (`Replayer::ground_military_hand` -- "grant a player the card they are
///   observed to play", the same idiom every other journal-named play in
///   this file already uses). This is not a guess: the age I/II/III bonus
///   cards are the ONLY cards with a nonzero `defense_bonus`, one value
///   each, so the printed number alone is already the card's full
///   identity -- whether or not this binary's fictional simulated hand
///   happened to deal that exact card is irrelevant, same as it is
///   irrelevant for a `PlayAggression`/`DeclareWar`/`ProposePact`/
///   `PlayTactic` line naming a card the simulated deal never dealt.
/// - `Flat`: any currently-legal `Move::Defend` candidate with
///   `defense_bonus == 0` qualifies (every non-`Bonus` military-deck card
///   defends for the same flat +1, `interact::defense_points`). If the
///   simulated hand has one, `r.discard_solver` picks among them exactly as
///   it does for a forced hand-limit discard (same underlying fact: a
///   specific card permanently leaves the hand), so the same solved/
///   chosen/forced-collision honesty applies. If it has NONE (a small
///   simulated hand can, by chance, be all `Bonus` cards -- seen in the
///   real corpus), [`flat_defense_filler`] grounds one: since identity
///   cannot affect any observable outcome here, this cannot be a wrong
///   guess in the sense the rest of this file guards against, only an
///   arbitrary bookkeeping label.
///
/// Every `Move::Defend`/`Move::DefendDone` here is an auto-resolution
/// (`record: false`), matching how a 0-committed defense was already
/// treated before this function could see any committed cards at all --
/// see `try_apply`'s doc for what `record` distinguishes.
fn resolve_aggression_defense(r: &mut Replayer, next_text: Option<&str>) -> Result<(), MismatchKind> {
    if !matches!(r.state.pending.top(), Some(Pending::Defense(_))) {
        return Ok(());
    }
    let clauses = next_text.and_then(parse_defense_clauses).unwrap_or_default();
    for clause in clauses {
        let Some(Pending::Defense(d)) = r.state.pending.top() else {
            return Err(MismatchKind::StuckPending(
                "aggression defense: journal names a committed card after the pending defense already closed".into(),
            ));
        };
        let player = d.player;
        let card = match clause {
            DefenseClause::Bonus(bonus) => {
                let id = defense_bonus_card(bonus);
                r.ground_military_hand(player, id);
                id
            }
            DefenseClause::Flat => {
                let flat: Vec<CardId> = legal::legal_moves(&r.state)
                    .as_slice()
                    .iter()
                    .filter_map(|mv| match mv {
                        Move::Defend { card } if card.get().effects.defense_bonus == 0 => Some(*card),
                        _ => None,
                    })
                    .collect();
                if flat.is_empty() {
                    let filler = flat_defense_filler(r.state.age_military);
                    r.ground_military_hand(player, filler);
                    filler
                } else {
                    let (idx, certainty) = r.discard_solver.choose(player, r.current_lineno, &flat);
                    if std::env::var("REPLAY_DEBUG_ALL").is_ok() {
                        eprintln!(
                            "DEBUG aggression defense: player {player} line {} picked {} of {} flat candidates ({certainty:?})",
                            r.current_lineno,
                            flat[idx].get().name,
                            flat.len()
                        );
                    }
                    flat[idx]
                }
            }
        };
        r.try_apply(Move::Defend { card }, false)?;
    }
    if matches!(r.state.pending.top(), Some(Pending::Defense(_))) {
        r.try_apply(Move::DefendDone, false)?;
    }
    Ok(())
}

/// The unique card that prints `defense_bonus == bonus` (2, 4 or 6 -- one
/// per age I/II/III; `card_table.rs` has exactly one `Bonus`-type card per
/// value). Panics on any other input, which would mean `parse_defense_
/// clauses` parsed a bonus number BGO's own client never actually prints.
fn defense_bonus_card(bonus: i16) -> CardId {
    (0..crate::CARDS.len() as u16)
        .map(CardId)
        .find(|id| id.kind() == CardType::Bonus && id.get().effects.defense_bonus == bonus)
        .unwrap_or_else(|| panic!("no Bonus card prints defense_bonus {bonus}"))
}

/// A filler `CardId` for a `DefenseClause::Flat` commit whose simulated hand
/// holds no zero-`defense_bonus` card at all. Every non-`Bonus` military-
/// deck card type (`Tactic`/`Aggression`/`War`/`Pact`/`Territory`/`Event`)
/// defends for the same flat +1 (`interact::defense_points`), so which one
/// is picked cannot affect any observable outcome, only bookkeeping --
/// mirrors `event_plan::unused_card_of_level`'s own "identity does not
/// matter, but pick something real and age-plausible" cascade. Prefers
/// `age` (the military deck's current age) so the filler is not
/// anachronistic; falls back to any age if that age has no such card.
fn flat_defense_filler(age: crate::Age) -> CardId {
    let is_flat_military_kind = |k: CardType| {
        matches!(k, CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Territory | CardType::Event)
    };
    let ids = (0..crate::CARDS.len() as u16).map(CardId);
    ids.clone()
        .find(|id| is_flat_military_kind(id.kind()) && id.get().age == age)
        .or_else(|| ids.clone().find(|id| is_flat_military_kind(id.kind())))
        .expect("the card table has at least one non-Bonus military-deck card")
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
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::CardList;
    use crate::CardType;

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
    fn a_take_line_with_no_uses_clause_at_all_cost_zero_actions_not_an_unknown_cost() {
        // Hammurabi's printed leader-take discount cancels the 1 CA of a
        // leader in one of the five cheapest row slots, and BGO then prints
        // no cost clause at all. 333 such lines in the corpus, every one
        // with Hammurabi in play -- see `observed_take_cost`.
        assert_eq!(observed_take_cost("Orange takes Michelangelo in hand"), 0);
        assert_eq!(observed_take_cost("Orange takes Alchemy in hand Orange uses 2 civil action"), 2);
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
    fn color_after_finds_the_actor_on_alexanders_leaderless_death_line() {
        // `replay_game`'s `RemoveLeaderYellow` dispatch relies on this same
        // helper reading the actor out of the line's OWN trailing clause,
        // since (unlike almost every other action line) the text carries no
        // leading colour at all.
        let text = "Alexander dies after building his great Empire Orange gets 1 yellow token";
        assert_eq!(color_after(text, "Empire "), Some(Color::Orange));
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

    /// REPLAYER FIX (`docs/REPLAY.md` fifth pass): Reserves' `FoodOrRes`
    /// pick is glued onto the SAME row as the "plays Reserves" line, not a
    /// standalone row -- `trailing_produces` reads it from anywhere in the
    /// text, unlike `parse_standalone_produces` which requires the WHOLE
    /// line to be nothing else.
    #[test]
    fn trailing_produces_reads_a_produces_clause_glued_onto_a_play_line() {
        assert_eq!(trailing_produces("Orange plays Reserves Orange produces 2 resources"), Some((true, 2)));
        assert_eq!(trailing_produces("Orange plays Reserves Orange produces 3 food"), Some((false, 3)));
    }

    #[test]
    fn trailing_produces_is_none_with_no_produces_clause_at_all() {
        assert_eq!(trailing_produces("Purple builds Bronze Purple spends 2 resources"), None);
    }

    #[test]
    fn trailing_gets_science_reads_breakthroughs_own_bonus_clause() {
        // The age signal `resolve_named_card_by_effect` matches Breakthrough
        // siblings against: 2 for the Age I copy, 3 for Age II
        // (`sources/bga_throughtheages_material.inc.php`).
        let text = "discovers Iron using Breakthrough Orange loses 5 science; Orange gets 2 science";
        assert_eq!(trailing_gets_science(text), Some(2));
        let text3 = "revolutions using Breakthrough Change government to Constitutional Monarchy; \
                     6 science points spent; Grey loses 6 science; Grey gets 3 science";
        assert_eq!(trailing_gets_science(text3), Some(3));
    }

    #[test]
    fn trailing_gets_science_ignores_an_unrelated_gets_clause() {
        // `"gets N civil action"` (a leader/event grant) must not be
        // mistaken for Breakthrough's science bonus.
        assert_eq!(trailing_gets_science("Orange elects Hammurabi Orange gets 1 civil action"), None);
    }

    #[test]
    fn correct_hand_family_swaps_a_wrong_age_sibling_for_the_evidence_backed_one() {
        // The scenario `free_civil_action_move`/`ActionClass::PlayActionCard`
        // hit for real (`7523044`, `7522665`): an earlier `TakeCard` line
        // guessed Frugality's Age A copy (`best_age_sibling`'s necessarily
        // age-blind default), but THIS line's own `"produces 2 food"` proves
        // it was really the Age I copy -- the hand entry has to be corrected,
        // not just the move about to be played.
        let mut state = game::new_game(2, 1);
        let p = &mut state.players[0];
        p.hand_civil = CardList::new();
        p.hand_civil.push(CardId::by_name("Frugality (A)").unwrap());
        correct_hand_family(p, CardId::by_name("Frugality (I)").unwrap());
        assert!(p.hand_civil.contains(CardId::by_name("Frugality (I)").unwrap()));
        assert!(!p.hand_civil.contains(CardId::by_name("Frugality (A)").unwrap()));
    }

    #[test]
    fn correct_hand_family_is_a_no_op_once_the_right_card_is_already_in_hand() {
        let mut state = game::new_game(2, 1);
        let p = &mut state.players[0];
        p.hand_civil = CardList::new();
        p.hand_civil.push(CardId::by_name("Frugality (II)").unwrap());
        correct_hand_family(p, CardId::by_name("Frugality (II)").unwrap());
        assert_eq!(p.hand_civil.as_slice(), &[CardId::by_name("Frugality (II)").unwrap()]);
    }

    #[test]
    fn spent_resources_reads_the_discounted_amount_on_a_using_line() {
        assert_eq!(
            spent_resources("Purple builds Printing Press using Urban Growth Purple spends 2 resources"),
            Some(2)
        );
    }

    /// REPLAYER FIX (`docs/REPLAY.md` fifth pass): a unit build/upgrade's
    /// `p.mil_discount`-funded portion is printed as a SEPARATE `"loses N
    /// military resource"` clause, not folded into `"spends"` -- reading
    /// `spends` alone silently under-counted the total by exactly this much.
    #[test]
    fn lost_military_resource_reads_a_loses_military_resource_clause() {
        assert_eq!(
            lost_military_resource("Purple builds Warrior Purple loses 1 military resource; Purple spends 1 resource"),
            Some(1)
        );
        assert_eq!(
            lost_military_resource("Green builds Warrior Green loses 2 military resource"),
            Some(2)
        );
    }

    #[test]
    fn lost_military_resource_ignores_an_unrelated_loses_clause() {
        // "loses 1 population" (Reign of Terror etc.) must not be mistaken
        // for a military-resource clause just because both start with " loses ".
        assert_eq!(lost_military_resource("Purple loses 1 population"), None);
        assert_eq!(lost_military_resource("Purple builds Bronze Purple spends 2 resources"), None);
    }

    /// The property this binary's build-cost cross-check actually needs:
    /// the TOTAL a unit build paid is `loses` + `spends` summed, matching
    /// the engine's own `costs::build_cost_for` (which prices units at full
    /// printed cost, before `p.mil_discount` is netted off at apply time) --
    /// confirmed against real BGO games where reading `spends` alone made
    /// this binary wrongly report a "build cost mismatch (unmodeled
    /// discount)" for a cost that was actually correct once both clauses are
    /// counted.
    #[test]
    fn total_paid_for_build_sums_the_military_resource_and_spends_clauses() {
        assert_eq!(
            total_paid_for_build("Purple builds Warrior Purple loses 1 military resource; Purple spends 1 resource"),
            Some(2)
        );
        assert_eq!(
            total_paid_for_build("Purple builds Swordsmen Purple loses 1 military resource; Purple spends 2 resources"),
            Some(3)
        );
        // A fully mil-discount-funded build has no "spends" clause at all.
        assert_eq!(total_paid_for_build("Green builds Warrior Green loses 2 military resource"), Some(2));
        // An ordinary building has neither clause matching "loses ... military
        // resource" -- falls back to plain `spends`.
        assert_eq!(total_paid_for_build("Purple builds Bronze Purple spends 2 resources"), Some(2));
        // Neither clause present at all (e.g. a fully free build): None.
        assert_eq!(total_paid_for_build("Purple builds 1 stage of Pyramids"), None);
    }

    #[test]
    fn parse_defense_clauses_reads_a_single_bonus_card_and_skips_the_strength_trailer() {
        let text = "Orange defends 1 Defense card +6 played; Orange strength: 26; Purple strength: 26";
        assert_eq!(parse_defense_clauses(text), Some(vec![DefenseClause::Bonus(6)]));
    }

    #[test]
    fn parse_defense_clauses_reads_a_plain_military_card_as_flat() {
        let text = "Orange defends 1 military card played; Orange strength: 8; Purple strength: 8";
        assert_eq!(parse_defense_clauses(text), Some(vec![DefenseClause::Flat]));
    }

    #[test]
    fn parse_defense_clauses_reads_every_clause_on_a_multi_card_defense_in_order() {
        // Real corpus line: two age-matched bonus cards plus a flat card, in
        // one defense.
        let text = "Grey defends 1 Defense card +4 played; 1 Defense card +6 played; 1 military card played; Grey strength: 30; Green strength: 27";
        assert_eq!(
            parse_defense_clauses(text),
            Some(vec![DefenseClause::Bonus(4), DefenseClause::Bonus(6), DefenseClause::Flat])
        );
    }

    #[test]
    fn parse_defense_clauses_is_none_for_a_line_that_is_not_a_defends_line() {
        assert_eq!(parse_defense_clauses("Purple builds Bronze Purple spends 2 resources"), None);
    }


    /// REGRESSION (found by replaying the real BGO corpus at scale, game
    /// `7522949`): `civil_life_move` used to return `Some(Move::Pop)`
    /// whenever `one_time_discount.pop_food != 0`, with no food check at
    /// all -- and its caller applies the result via
    /// `apply::apply_free_civil_move` directly, bypassing
    /// `Replayer::try_apply`'s own `legal_moves` gate. An unaffordable Pop
    /// reached `apply::h_pop`'s internal `debug_assert!` with no prior
    /// legality check, hard-panicking the whole process (and losing every
    /// OTHER game's data in the same batch run) instead of producing an
    /// honest `Mismatch` the way every other unaffordable-move shape in
    /// this file does.
    #[test]
    fn civil_life_move_does_not_offer_pop_when_the_player_cannot_actually_afford_it() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        r.state.players[0].one_time_discount.pop_food = 1; // banked, but...
        r.state.players[0].food = 0; // ...not enough food to spend it
        assert_eq!(civil_life_move(&r, 0, ActionClass::IncreasePopulation, None), None);
    }

    #[test]
    fn civil_life_move_offers_pop_when_the_player_can_actually_afford_it() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        r.state.players[0].one_time_discount.pop_food = 1;
        r.state.players[0].food = 20; // plenty
        assert_eq!(civil_life_move(&r, 0, ActionClass::IncreasePopulation, None), Some(Move::Pop));
    }

    /// `is_pure_confirmation_line`'s membership is what routes `PlayEvent`,
    /// `WinAuction`, and `Colonize` lines around `resolve_intervening`
    /// REGRESSION (real BGO game `7523818`, and 94 others like it in the
    /// 1,011-game corpus -- `bid_ceiling_mismatch`'s own doc comment).
    /// Player 0 starts (`game::new_game`, round 1, before any end-of-turn
    /// draw) with exactly one Warriors worker and an empty military hand --
    /// `interact::max_force` computes exactly 1 for them. A bid of 3
    /// against a standing high bid of 2 is a real raise this binary cannot
    /// afford under its own reconstructed state, but it must not be
    /// reported as the same `IllegalMove` an actual engine defect would
    /// produce: the true cause this binary can never rule out is a military
    /// bonus card sitting unplayed (and therefore unidentified) in the
    /// bidder's hand.
    #[test]
    fn a_bid_that_exceeds_this_binarys_own_force_ceiling_is_reported_as_hidden_info_not_illegal_move() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        r.state.phase = Phase::Actions;
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        assert_eq!(
            crate::interact::max_force(&r.state, &r.state.players[0]),
            1,
            "fixture assumption: a fresh player 0 can send exactly their starting Warrior"
        );
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[0, 1], 0, 2, Some(1), 0)));

        let result = apply_one(&mut r, 0, ActionClass::Bid, None, "bids 3", "Orange bids 3", None);

        assert!(
            matches!(result, Err(MismatchKind::UnrecoverableHiddenInfo(_))),
            "expected UnrecoverableHiddenInfo, got {result:?}"
        );
    }

    /// Companion to the test above: reverting `bid_ceiling_mismatch` to
    /// `None` (i.e. deleting the reclassification and always keeping
    /// `try_apply`'s own `IllegalMove`) must turn this same fixture back
    /// into a plain `IllegalMove` -- confirming the test actually exercises
    /// the new code path rather than passing for an unrelated reason.
    #[test]
    fn without_the_reclassification_the_same_fixture_would_be_a_bare_illegal_move() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        r.state.phase = Phase::Actions;
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[0, 1], 0, 2, Some(1), 0)));

        let result = r.try_apply(Move::Bid { n: 3 }, true);

        assert!(matches!(result, Err(MismatchKind::IllegalMove { .. })), "expected a bare IllegalMove, got {result:?}");
    }

    /// entirely in `replay_game`'s main loop -- this pins the exact set so
    /// a future edit that silently drops one back into the "must call
    /// resolve_intervening" path is caught here rather than only as a
    /// re-regression in the full corpus run.
    #[test]
    fn is_pure_confirmation_line_is_true_only_for_play_event_win_auction_and_colonize() {
        assert!(is_pure_confirmation_line(ActionClass::PlayEvent));
        assert!(is_pure_confirmation_line(ActionClass::WinAuction));
        assert!(is_pure_confirmation_line(ActionClass::Colonize));
        assert!(!is_pure_confirmation_line(ActionClass::Pass));
        assert!(!is_pure_confirmation_line(ActionClass::Bid));
        assert!(!is_pure_confirmation_line(ActionClass::Discard));
    }

    /// REGRESSION (found by replaying real BGO games `7522652`/`7523072`):
    /// pins the exact failure mode `is_pure_confirmation_line` exists to
    /// avoid. A colonize auction still has an active bidder (`player: 0`)
    /// who has not yet bid or passed -- the real journal's next line is
    /// THEIR OWN explicit "passes"/"bids" text, but BGO prints the "X wins"
    /// confirmation line first, naming a DIFFERENT player (the eventual
    /// winner) as its actor. If `resolve_intervening` is (wrongly) called
    /// for that confirmation line -- i.e. if a future edit ever removes
    /// `WinAuction` from `is_pure_confirmation_line` -- `decider() (0) !=
    /// expected_actor (1)` sends it straight to the generic
    /// `Pending::Auction` auto-drain fallback, which synthesizes a FAKE
    /// `Move::BidPass` for player 0 on the spot: this test confirms that is
    /// exactly what the fallback does when reached directly, which is why
    /// `replay_game`'s main loop must never reach it for a `WinAuction`
    /// line at all (skipping the call, as `is_pure_confirmation_line`
    /// makes it do, leaves the auction's `player: 0` genuinely pending for
    /// their own real, upcoming line to resolve instead).
    #[test]
    fn resolve_intervening_auto_drains_a_still_open_auction_with_a_fake_bid_pass_when_called_for_a_different_expected_actor(
    ) {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        // Mirrors the real shape found on `7522652`: player 1 already placed
        // the high bid (4), and player 0 (`active[1]`, `pos: 1`) is the
        // still-outstanding decider -- if they ALSO pass, player 1 becomes
        // the sole active bidder holding the high bid and wins outright.
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[1, 0], 1, 4, Some(1), 0)));
        r.state.phase = Phase::Actions;
        assert_eq!(r.state.decider(), 0); // player 0's own bid/pass is still outstanding

        // Called (wrongly) as if resolving a path toward player 1 -- the
        // shape a `WinAuction` line naming the eventual winner would create
        // if it were not excluded from this call entirely.
        let result = r.resolve_intervening(1, (ActionClass::WinAuction, Some(territory)), false);

        assert!(result.is_ok());
        // Player 0's own decision was fabricated and consumed sight-unseen:
        // with only player 1 left active, the auction auto-resolves them as
        // the winner and immediately opens THEIR colonize pending -- there
        // is no longer anything for player 0's real, still-unread
        // "passes"/"bids" line to apply to. Exactly the bug this file's
        // `is_pure_confirmation_line` exclusion prevents in the real
        // per-line loop.
        assert_ne!(r.state.decider(), 0);
        assert!(matches!(r.state.pending.top(), Some(Pending::Colonize(_))));
    }

    /// REGRESSION (the whole point of `event_plan`; the shape that broke
    /// real 2p game `7522647`, where "Development of Science" fired at
    /// round 4 instead of round 10 because a preparation was invented for a
    /// player who had simply passed). A Politics decision by a player whose
    /// journal shows NO `"plays event"` line of their own here must not
    /// touch the preparation queue at all.
    #[test]
    fn a_politics_decision_by_a_player_the_journal_never_shows_preparing_consumes_no_event() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 1, "Purple plays event Purple scores 1 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new());
        r.current_lineno = 10; // well past the one preparation's own line
        // `new_game` opens in the Action phase (round 1 has no politics);
        // this is the ordinary later-round shape.
        r.state.phase = Phase::Politics;
        assert_eq!(r.state.decider(), 0);

        r.resolve_political_decision(0).expect("player 0 simply passes");

        // The queue is untouched and no event resolved: the preparation on
        // line 5 belongs to player 1, and stays theirs.
        assert_eq!(r.next_prep, 0);
        assert!(r.state.past_events.as_slice().is_empty());
        assert_eq!(r.state.players[0].culture, 0);
        assert_eq!(r.state.phase, Phase::Actions);
    }

    /// The other half of the same property: the player the journal DOES
    /// name prepares exactly the solved card, reveals exactly the card the
    /// journal's own `"Current event:"` clause names, and scores exactly
    /// the culture BGO logged (which is where the prepared card's age was
    /// read from in the first place).
    #[test]
    fn the_player_the_journal_names_prepares_the_solved_card_and_reveals_the_logged_one() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 2 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let prepared = plan.preparations[0].card;
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;

        r.resolve_political_decision(0).expect("player 0's own logged preparation");

        assert_eq!(r.next_prep, 1);
        assert_eq!(r.state.past_events.as_slice(), &[card_index["Development of Settlement"]]);
        assert_eq!(r.state.future_events.as_slice(), &[prepared]);
        assert_eq!(r.state.players[0].culture, 2);
    }

    /// REGRESSION (BGO game `7522650`): the `"plays event"` line is skipped
    /// as a confirmation, so the preparation it records is applied at the
    /// NEXT line -- and with Julius Caesar armed, that next line is
    /// routinely the same player's own `"passes Political Phase"` for the
    /// declined SECOND action. The explicit-political-line fast path used to
    /// win, applying the pass as the player's FIRST political action and
    /// stranding the preparation at the head of the queue, which then
    /// blocked every later event in the game.
    #[test]
    fn an_owed_preparation_outranks_this_players_own_explicit_pass_line() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new());
        r.state.players[0].leader = crate::CardId::by_name("Julius Caesar").expect("Caesar is in the table");
        r.state.phase = Phase::Politics;
        r.current_lineno = 6; // the "plays event" line on 5 has gone by

        // `true` = the line about to be translated is player 0's own
        // explicit "passes Political Phase".
        r.resolve_intervening(0, (ActionClass::Pass, None), true).expect("resolvable");

        assert_eq!(r.next_prep, 1, "the preparation is applied, not skipped past");
        assert_eq!(r.state.past_events.as_slice(), &[card_index["Development of Settlement"]]);
        // Caesar leaves the phase open, so the pass line still has its own
        // real decision to land on.
        assert_eq!(r.state.phase, Phase::Politics);
    }

    /// Julius Caesar offers a SECOND political action after the first one
    /// (`apply::end_politics`), and a human who declines it leaves BGO's
    /// `"passes Political Phase"` line wherever they happened to click it --
    /// which is routinely AFTER some of their own Action-phase lines, since
    /// BGO does not make them answer before acting. The engine does: those
    /// action lines are illegal until politics closes. So the pass is
    /// applied first and its journal line is a late confirmation.
    #[test]
    fn a_passes_line_that_arrives_after_this_file_already_passed_for_that_player_is_a_confirmation() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        r.state.phase = Phase::Politics;
        r.resolve_political_decision(0).expect("nothing in the plan, so a pass");
        assert_eq!(r.auto_passed[0], 1);
        assert_eq!(r.state.phase, Phase::Actions);

        // The journal's own line for that same pass, read later.
        let out = apply_one(&mut r, 0, ActionClass::Pass, None, "passes Political Phase", "Orange passes Political Phase", None);

        assert!(out.is_ok(), "the pass this file already applied is confirmed, not re-applied");
        assert_eq!(r.auto_passed[0], 0, "and it is consumed, so a SECOND stray passes line still stops the game");
        assert!(matches!(
            apply_one(&mut r, 0, ActionClass::Pass, None, "passes Political Phase", "Orange passes Political Phase", None),
            Err(MismatchKind::IllegalMove { .. })
        ));
    }

    /// REGRESSION (found by replaying real BGO games `7522669`/`7523025`):
    /// pins the other failure mode `is_pure_confirmation_line` avoids for
    /// `Colonize`. Unlike `WinAuction`, nothing is left pending here at all
    /// by the time a `"<Color> colonizes ..."` confirmation line is
    /// reached -- the winner's whole colonize sequence already ran to
    /// completion synchronously as a side effect of the auction's own last
    /// `Bid`/`BidPass` (`interact::auction_move` -> `colonize`, both
    /// auto-resolving single-option decisions), and control already
    /// returned to whoever's turn triggered the auction in the first place
    /// (`state.current`), not to the colonizer. If `resolve_intervening`
    /// were (wrongly) called for that confirmation line -- i.e. if a future
    /// edit ever removes `Colonize` from `is_pure_confirmation_line` -- it
    /// falls straight to the generic "no pending, decider != expected
    /// actor" error with nothing left to auto-drain at all, exactly the
    /// `StuckPending` mismatch this bucket was named for.
    #[test]
    fn resolve_intervening_reports_a_stuck_pending_for_a_colonize_confirmation_line_once_control_has_already_returned_to_a_different_player(
    ) {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new());
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        // Player 1's own auction win and colonize sequence are both already
        // fully resolved (no `Pending` left at all); `state.current` has
        // returned to player 0, whose turn triggered the auction.
        r.state.current = 0;
        r.state.phase = Phase::Actions;
        assert!(r.state.pending.is_empty());
        assert_eq!(r.state.decider(), 0);

        // Called (wrongly) as if resolving a path toward player 1, the
        // confirmation line's own named colonizer -- the shape a `Colonize`
        // line would create if it were not excluded from this call at all.
        let result = r.resolve_intervening(1, (ActionClass::Colonize, Some(territory)), false);

        assert!(result.is_err());
    }
}
