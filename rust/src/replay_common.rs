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
//! - **Colonization sacrifice specifics** used to be listed here as an
//!   unimplemented approximation ("this file auto-drains colonization by
//!   picking the engine's own first offered option at each step until the
//!   force clears"). It is now driven from the journal, exactly like an
//!   aggression defense: `"Sacrificed Units:; 1 Warrior; 1 Colonization
//!   card +2; ..."` is one clause PER committed piece, a unit type
//!   (unique per age) or a printed colonization bonus (1/2/3, one card per
//!   age I/II/III). See [`SacrificeClause`], [`prescan_colonize_
//!   sacrifices`] and [`Replayer::drain_colonize`]. Only James Cook's
//!   `"1 Military card +1"` discard clause leaves its card unnamed, and
//!   only the COUNT of those is claimed.
//!
//!   The approximation was not cosmetic. The auto-drain spent whatever the
//!   SIMULATED hand happened to hold, so a human force of "one Knight plus
//!   a +3 bonus card" was reproduced as four sacrificed Warriors -- units
//!   permanently gone from a board this file otherwise tracks exactly, and
//!   with them the player's later military strength, their colonization
//!   ceiling, and every bid they went on to make. [`Replayer::
//!   approximate_colonize`] survives as the fallback for the residual ~2%
//!   the journal's own list cannot be applied to, and still flags the game.

use std::collections::{HashMap, VecDeque};

use crate::corpus::{
    actor_and_rest, best_age_sibling, classify, family_siblings, longest_known_card_prefix, ActionClass, Classified,
    Color, GameMeta, LineOutcome,
};
pub use crate::corpus::build_card_index;
use crate::discard_solver::{DiscardSolver, FutureNeed};
use crate::event_plan::EventPlan;
use crate::moves::{ChurchillChoice, PactSide};
use crate::state::{
    Choice, ChoiceKind, ChoiceOption, GameState, Keyword, Pending, PlayerState, Phase, TechSlot, MAX_HAND, MAX_PLAYERS,
};
use crate::{apply, costs, economy, effects, game, legal, CardId, CardType, Move};

/// One journal-observed resolution of a single War over Technology spoils
/// pick -- see `Replayer::war_tech_spoils`'s doc for the journal shape each
/// variant comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WarSpoil {
    /// `"<Color> steals <Card> from <Color>"`.
    Steal(CardId),
    /// `"<Color> gets N science; <Color> loses N science"` -- the amount
    /// itself is not needed here: `interact::take_war_science` recomputes it
    /// deterministically from the live `budget`/loser's science, exactly like
    /// every other amount-bearing spoils line this file resolves by SHAPE,
    /// not by re-deriving the engine's own arithmetic from journal text.
    Science,
}

// ---------------------------------------------------------------------
// Journal line
// ---------------------------------------------------------------------

/// One journal row, still borrowing from the file's text.
struct Line<'a> {
    lineno: usize,
    /// Column 2, `player_colour` -- BGO's OWN attribution of the line,
    /// present on every row (verified: this is the "the data was on every
    /// line all along" shape, `docs/REPLAY.md`'s eighth pass). Kept as the
    /// raw field rather than a parsed `Color` because a handful of rows
    /// (system messages) print something other than a colour here, and
    /// only the callers that actually need it (currently just the
    /// no-leading-colour `ColumbusColonize` shape) should have to decide
    /// what to do about that.
    color: &'a str,
    age: &'a str,
    round: &'a str,
    text: &'a str,
}

/// Column 3 of the journal (`Line::age`) spells the age exactly like
/// [`crate::cards::Age`]'s own variant names -- checked against every
/// distinct value in the full 1,011-game corpus (`cut -f3 *.tsv | sort
/// -u`: `A`, `I`, `II`, `III`, `IV`, plus the header row, which never
/// reaches here). `None` for anything else rather than a guess -- an
/// unrecognised value should not silently skip the age catch-up below.
fn parse_age(s: &str) -> Option<crate::cards::Age> {
    use crate::cards::Age;
    match s {
        "A" => Some(Age::A),
        "I" => Some(Age::I),
        "II" => Some(Age::II),
        "III" => Some(Age::III),
        "IV" => Some(Age::IV),
        _ => None,
    }
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
        out.push(Line {
            lineno: i + 1,
            color: fields[1],
            age: fields[2],
            round: fields[3],
            text: fields[4],
        });
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
// Move category -- shared by `bin/agreement.rs` and `bin/agreefit.rs`
// ---------------------------------------------------------------------

/// The nine buckets this project's phase-1 brief names, plus `Other` for
/// every `Move` variant that doesn't fit one cleanly. Deliberately an
/// exhaustive match over `Move` (no wildcard arm) in [`categorize`]: a new
/// `Move` variant must be placed here explicitly rather than silently
/// falling into `Other`.
///
/// Moved here from `bin/agreement.rs` (which introduced it) when
/// `bin/agreefit.rs` needed the identical labelling for its own per-decision
/// cache records -- a category label is a small, pure function of
/// `(pending, Move)`, not part of either binary's own replay-walking or
/// feature-extraction machinery, but two binaries hand-maintaining two
/// copies of "which bucket does `Move::Take` fall in" is exactly the kind of
/// drift this project's own house style forbids, so it lives once, in the
/// library, alongside the [`Decision`] type both binaries already share.
///
/// `Build` covers `Move::Build` AND `Move::Develop`/`Move::Upgrade` --
/// developing a technology or upgrading a farm/mine/unit is a building
/// action under TTA's rules (§3, §4), not a structurally different
/// decision, so folding them in gives `build` its true volume instead of
/// splintering it across three buckets.
///
/// `Bid` is a first-class category, split out of `Other`, specifically
/// BECAUSE `docs/HUMAN_PLAY.md`'s census flags "4p bot barely contests
/// colonization auctions" as a live, named finding -- that claim can only be
/// tested at the move level if `Bid`/`BidPass` decisions are visible as
/// their own bucket rather than buried in `Other` alongside unrelated
/// things.
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
/// `apply::apply` directly, never through the recorded `try_apply` path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
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
    pub fn name(self) -> &'static str {
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
/// (accepting a pact); every other reachable kind falls to `Other`.
pub fn categorize_choice(pending: Option<&Pending>) -> Category {
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
            | ChoiceKind::PlunderSplit { .. }
            | ChoiceKind::FoodOrResSplit { .. } => Category::Other,
        },
        // A recorded `Move::Choose` always has an open `Pending::Choice` at
        // record time (`try_apply` only records after its own legality
        // check passes, and `Choose` is never legal without one) -- this
        // arm is unreachable in practice, kept only as a defensive
        // fallback rather than a `panic!`.
        _ => Category::Other,
    }
}

pub fn categorize(pre_move_pending: Option<&Pending>, mv: Move) -> Category {
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
    /// Per-seat count of `resolve_political_decision`'s own "no disposable
    /// filler exists" wash, still owed a real hand_military decrement --
    /// see that function's own doc and [`Self::repay_military_hand_
    /// deficit`], which drains this the next time this seat's hand
    /// genuinely grows.
    military_hand_deficit: [u32; 4],
    /// Whether any colonization in this game was resolved by the
    /// approximate auto-drain rather than a verified sacrifice match.
    colonize_approximated: bool,
    /// How many SIMULATED filler cards this game had to be converted into
    /// military bonus cards to make a journal-logged bid legal -- see
    /// [`Replayer::ground_bid_ceiling`]. Reported rather than swallowed:
    /// each one is a hand slot whose identity was deduced from a bid rather
    /// than read off a line naming the card.
    bid_ceilings_grounded: u32,
    /// How many journal-observed `Take`s this REPLAYER accepted despite
    /// `costs::take_gate`'s `hand_full` gate rejecting them -- see
    /// [`take_blocked_only_by_hand_full`] and `docs/REPLAY.md`'s Take/
    /// HandFull "genuinely unexplained discrepancy" conclusion. Reported
    /// rather than swallowed, right next to `bid_ceilings_grounded`: each
    /// one is a place this file knowingly diverges from self-play legality
    /// to reproduce what the real BGO implementation actually permitted.
    hand_full_takes_overridden: u32,
    /// Every `"<Color> colonizes ..."` line's own sacrifice list, in journal
    /// order -- see [`prescan_colonize_sacrifices`]. The front is the
    /// outcome of the auction currently in progress; it is popped when that
    /// colonization is driven ([`Replayer::drain_colonize`]) and peeked at
    /// while the auction is still open, to ground the winner's hidden bonus
    /// cards before `interact::colonize` snapshots their hand into the
    /// `Pending::Colonize` pools ([`Replayer::ground_auction_winner_hand`]).
    colonize_sacrifices: VecDeque<ColonizeSacrifice>,
    /// Number of actionable (non-bookkeeping) journal lines consumed.
    actions_consumed: usize,
    /// `Line::lineno`s of `"<Color> discards N card(s)"` lines that are
    /// BGO's redundant second log of the SAME end-of-turn discard already
    /// resolved by this reconstruction's own preceding `End turn` line --
    /// see `replay_game`'s main-loop `ActionClass::Discard` special case
    /// (the `EndTurn` dispatch arm's sibling, right before it) for the
    /// full mechanism and the real game this was found on. The main loop
    /// skips these lines as pure confirmations instead of translating
    /// them; `GameResult` surfaces the count so a corpus run can verify
    /// the skip fired exactly where the journal's own timestamps say it
    /// should.
    trailing_discards_lines: Vec<usize>,
    /// Journal `Line::lineno`s of `"<Color> builds <Card>"` /
    /// `"<Color> builds <N> stage(s) of <Wonder>"` lines that are BGO's
    /// redundant SECOND log of the SAME build already applied by this
    /// reconstruction's own preceding, identical build line for the same
    /// actor in the same turn -- see `replay_game`'s main-loop
    /// `BuildBuilding`/`BuildUnit` special case (the `EndTurn` dispatch arm's
    /// sibling, right before `apply_one` runs) and `apply_one`'s
    /// `BuildWonderStage` arm for the full mechanism and the real games this
    /// was found on (e.g. `7522652`'s 4x "builds Movies" where BGO's own
    /// end-of-turn resource math proves only 3 happened, and `7522432`'s
    /// 2x "builds 1 stage of Fast Food Chains"). The replayer skips these
    /// lines as pure confirmations instead of charging the build's resources
    /// twice (the over-spend that starved a later, genuinely-unaffordable
    /// build/wonder stage into `IllegalMove: Build`/`WonderStep`);
    /// `GameResult` surfaces the count so a corpus run can verify the skip
    /// fired exactly where the journal's own timestamps say it should.
    duplicate_build_lines: Vec<usize>,
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
    /// Per-attacker FIFO of `(food, resources)` splits pulled off every
    /// journal-observed Plunder resolution line -- see
    /// `prescan_plunder_splits`'s doc and `resolve_intervening`'s
    /// `ChoiceKind::PlunderSplit` handling, which drains it.
    plunder_splits: HashMap<u8, VecDeque<(i16, i16)>>,
    /// Per-actor FIFO of `(food, resources)` splits pulled off every
    /// journal-observed Foray/Raiders "and/or" grant resolution line -- see
    /// `prescan_produces_grants`'s doc and `resolve_political_decision`'s
    /// `PrepareEvent` handling, which corrects `events::food_or_resources`'s
    /// deterministic guess against it. Set directly by [`replay_game`] after
    /// construction (like `record_decisions`), not threaded through
    /// [`Replayer::new`]'s own parameter list -- `Replayer::new` already has
    /// enough positional parameters that the ~40 test call sites in this
    /// file's own `#[cfg(test)]` module would all need touching for a
    /// twelfth; every test that doesn't care leaves this at its `Default`
    /// (empty), same as it would if this field didn't exist.
    produces_grants: HashMap<u8, VecDeque<(i16, i16)>>,
    /// [`Self::produces_grants`]'s LOSS-side mirror -- see
    /// `prescan_spends_grants`'s doc. Same "set directly after construction"
    /// convention, same reason.
    spends_grants: HashMap<u8, VecDeque<(i16, i16)>>,
    /// Per-attacker FIFO of `is_wonder` flags pulled off every
    /// journal-observed Infiltrate resolution line (`"concedes defeat"` OR
    /// `"Operation successful"`, both prefixes carry the same `"<Card> is
    /// killed"`/`"<Card> is destroyed"` shape -- see `prescan_infiltrates`'s
    /// doc and `resolve_intervening`'s `ChoiceKind::Infiltrate` handling,
    /// which drains it.
    infiltrates: HashMap<u8, VecDeque<bool>>,
    /// Per-actor FIFO of `(line index, card)` pulled off every
    /// journal-observed `"<Color> destroys <Card>"` line, pre-scanned once
    /// per game -- see `prescan_lose_pop_destroys`'s doc and
    /// `resolve_intervening`'s `ChoiceKind::LosePop` handling, which drains
    /// it ONLY for the out-of-journal-order case (a `LosePop` pending left
    /// open for a player who isn't `expected_actor`, e.g. opened as a side
    /// effect of a DIFFERENT player's political-phase event reveal). The
    /// `ChoiceKind::DestroyOwn` handling drains it the same way (an event's
    /// weakest-civilization destroy revealed on a DIFFERENT player's turn
    /// -- see that block's own doc). The ordinary same-line case
    /// (`c.player == expected_actor` and the upcoming line already IS that
    /// player's own `"destroys"` line) still defers to `apply_one`'s
    /// existing `Destroy | Disband` arm, exactly like before -- this
    /// FIFO/its `claimed_destroy_lines` companion exist purely to avoid
    /// double-applying a destroy line consumed early here.
    lose_pop_destroys: HashMap<u8, VecDeque<(usize, CardId)>>,
    /// Journal line INDICES (matching the main loop's own `journal.iter().
    /// enumerate()` index, not `Line::lineno`) already applied early by
    /// `resolve_intervening`'s `ChoiceKind::LosePop` out-of-order drain
    /// (see `lose_pop_destroys`) -- checked by `replay_game`'s main loop
    /// exactly like `putback_skips`, so that line is not translated AGAIN
    /// as an ordinary `Move::Destroy` once the main loop's own pointer
    /// reaches it.
    claimed_destroy_lines: std::collections::HashSet<usize>,
    /// Journal LINE NOS already handled by `resolve_intervening`'s `None`
    /// arm before the main loop's own `apply_one` runs: either a civil-life
    /// out-of-turn action applied via `apply_free_civil_move` (a political
    /// action banked a fresh `one_time_discount` after the main loop's own
    /// `civil_life_move` check ran), or an auto-resolved event destroy
    /// (single-option `DestroyOwn` choice resolved by `interact::push_choice`'s
    /// `auto` flag during the political phase's `run_queue`). The main loop
    /// skips `apply_one` for these lines to avoid double-applying.
    claimed_civil_life_lines: std::collections::HashSet<usize>,
    /// GLOBAL (not per-player -- Terrorism's own destruction line never
    /// names an attacker) FIFO of `CardId`s pulled off both journal shapes
    /// that resolve a `Pending::Choice(Raid)` -- the Terrorism event's own
    /// `"Terrorists destroy a <Color> <Building>"` and Aggression: Raid's
    /// `"Raid casualties ..."` -- see `prescan_raid_destroys`'s doc and
    /// `resolve_intervening`'s `ChoiceKind::Raid` handling, which drains it.
    raid_destroys: VecDeque<CardId>,
    /// Per-actor FIFO of territory `CardId`s pulled off every journal-
    /// observed `"<Color> loses <Territory> (<Age>)"` line -- the resolution
    /// of a REAL (multi-colony) `Pending::Choice(LoseColony)`, distinct from
    /// the single-colony auto-resolve glued onto the triggering event's own
    /// line -- see `prescan_lose_colonies`'s doc and `resolve_intervening`'s
    /// `ChoiceKind::LoseColony` handling, which drains it.
    lose_colonies: HashMap<u8, VecDeque<CardId>>,
    /// Per-actor FIFO of wonder `CardId`s pulled off every journal-observed
    /// `"Ravages of Time <Wonder> crumble(s)"` line -- the resolution of a
    /// REAL (multi-wonder) `Pending::Choice(FlipWonder)`, distinct from the
    /// single-wonder auto-resolve glued onto the triggering event's own line
    /// -- see `prescan_flip_wonders`'s doc and `resolve_intervening`'s
    /// `ChoiceKind::FlipWonder` handling, which drains it.
    flip_wonders: HashMap<u8, VecDeque<CardId>>,
    /// Per-actor FIFO of [`WarSpoil`]s pulled off every journal-observed
    /// `"<Color> takes spoils of war <Color> steals <Card> from <Color>"` /
    /// `"<Color> takes spoils of war <Color> gets N science; <Color> loses N
    /// science"` line -- the resolution of a `Pending::Choice(WarTech)`. War
    /// over Technology's spoils are explicitly MIXABLE (FAQ p.8, "some or
    /// all"): `interact::offer_war_tech` re-offers the choice with the
    /// advantage reduced by whatever was just stolen, so BGO logs one
    /// `"takes spoils of war"` line PER PICK, in the same order the choices
    /// were made -- e.g. `7522455` line 201 (`"steals Navigation"`) then line
    /// 202 (`"gets 6 science"`), a steal-then-remainder-as-science split. Set
    /// directly by `replay_game` after construction (like `produces_grants`),
    /// not threaded through `Replayer::new`'s own parameter list, for the
    /// same reason that field gives -- see `prescan_war_tech_spoils`'s doc
    /// and `resolve_intervening`'s `ChoiceKind::WarTech` handling, which
    /// drains it.
    war_tech_spoils: HashMap<u8, VecDeque<WarSpoil>>,
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
    /// Cross-validated `(actor seat, round) -> true hand-military excess`
    /// truth from [`prescan_discard_phase_oracle`] -- see that function's
    /// own doc and the module doc's "Discard-phase hand-size oracle"
    /// section. Set directly by [`replay_game`] after construction (like
    /// `produces_grants`), not threaded through [`Replayer::new`]'s own
    /// parameter list, for the same reason that field gives.
    discard_phase_oracle: HashMap<(u8, String), u32>,
    /// The FIRST `(actor, round)` checkpoint (in journal order) where this
    /// game's own reconstructed military-hand excess disagreed with
    /// [`discard_phase_oracle`]'s truth -- see
    /// [`Replayer::check_discard_phase_oracle`]. `None` either because the
    /// game never diverged, or because it stopped (a `Mismatch`) before any
    /// checkpoint disagreed.
    discard_oracle_divergence: Option<DiscardOracleDivergence>,
    /// How many `(actor, round)` checkpoints had a cross-validated journal
    /// entry to compare against (`discard_oracle_checked`) and how many of
    /// those this binary's own reconstruction matched exactly
    /// (`discard_oracle_agreed`) -- copied into [`GameResult`] at the end of
    /// [`replay_game`], the oracle's own "how much of the game is even
    /// checkable, and how much of THAT is right" coverage stat.
    discard_oracle_checked: u32,
    discard_oracle_agreed: u32,
    /// Text-only, per-(actor, round) `hand_military` size ledger from
    /// [`prescan_military_hand_ledger`] -- see that function's own doc. Set
    /// directly by [`replay_game`] after construction, same convention as
    /// `discard_phase_oracle`.
    military_hand_ledger: HashMap<(u8, String), LedgerCheckpoint>,
    /// This game's FIRST discard-phase-oracle divergence classified against
    /// [`military_hand_ledger`] -- see [`HandLedgerVerdict`] and
    /// [`GameResult::hand_ledger_verdict`]. Set at the same point
    /// `discard_oracle_divergence` is first set, never overwritten after.
    hand_ledger_verdict: Option<HandLedgerVerdict>,
    /// The last classified action line's [`ActionClass`], of ANY actor,
    /// strictly before the line currently being processed -- read (not yet
    /// overwritten by the current line) at the top of the main dispatch
    /// loop's `LineOutcome::Action` arm, then unconditionally overwritten
    /// with the current line's own class. This gives every checkpoint that
    /// reads it (the culture oracle below) "what happened right before this"
    /// without threading a parameter through every branch that can reach
    /// `EndTurn`.
    last_action_class: Option<ActionClass>,
    /// This game's FIRST culture-oracle divergence -- see
    /// [`CultureOracleDivergence`] and [`GameResult::culture_oracle_
    /// divergence`]. `None` either because the game's running culture total
    /// never drifted from BGO's own "(now M)" truth, or because it stopped
    /// (a `Mismatch`) before any checkpoint could.
    culture_oracle_divergence: Option<CultureOracleDivergence>,
    /// How many "End turn" checkpoints had a `"(now M)"` clause to compare
    /// against (`culture_oracle_checked`) and how many of those this
    /// binary's own reconstruction matched exactly (`culture_oracle_
    /// agreed`) -- copied into [`GameResult`], same coverage-stat
    /// convention as `discard_oracle_checked`/`discard_oracle_agreed`.
    culture_oracle_checked: u32,
    culture_oracle_agreed: u32,
    /// See [`PendingCultureCheck`]'s own doc: a culture-oracle comparison
    /// deferred past an `EndTurn` line whose production was blocked on a
    /// still-open discard decision. `None` the rest of the time (the common
    /// case: an ordinary `EndTurn` compares immediately, never touching this
    /// field at all).
    pending_culture_check: Option<PendingCultureCheck>,
    /// [`Replayer::culture_oracle_divergence`]'s science twin -- see
    /// [`ScienceOracleDivergence`]. `SCIENCE_ORACLE`-gated (unlike culture's
    /// always-on checkpoint): `None` for every game unless
    /// `debugflags::science_oracle()` is set, by construction, since
    /// `record_science_check` is the only writer and it is never called
    /// otherwise.
    science_oracle_divergence: Option<ScienceOracleDivergence>,
    /// [`Replayer::culture_oracle_checked`]/[`Replayer::culture_oracle_
    /// agreed`]'s science twin.
    science_oracle_checked: u32,
    science_oracle_agreed: u32,
    /// [`Replayer::pending_culture_check`]'s science twin -- see
    /// [`PendingScienceCheck`].
    pending_science_check: Option<PendingScienceCheck>,
    /// [`Replayer::culture_oracle_divergence`]'s resources twin -- see
    /// [`ResourceOracleDivergence`]. `RESOURCE_ORACLE`-gated, same
    /// never-written-unless-flagged guarantee as `science_oracle_divergence`.
    resource_oracle_divergence: Option<ResourceOracleDivergence>,
    resource_oracle_checked: u32,
    resource_oracle_agreed: u32,
    /// [`Replayer::pending_culture_check`]'s resources twin -- see
    /// [`PendingResourceCheck`].
    pending_resource_check: Option<PendingResourceCheck>,
    /// A `"GAME DATA UPDATED"` culture clause (`parse_game_data_updated_
    /// culture`'s own doc) not yet applied because its stated `old` did not
    /// match this reconstruction's live value the moment the line was read.
    /// Not a symptom of a wrong `old`: BGO's own admin correction can land
    /// while a war's spoils are themselves still deferred on OUR side (war
    /// resolution fires at `game::start_turn`, itself only reached once a
    /// blocked `EndTurn`'s own discard/production catches up -- the exact
    /// cascade `docs/REPLAY.md`'s "second-order artifact" section already
    /// documents for the culture-oracle checkpoint itself). Retried on every
    /// subsequent line (`flush_pending_game_data_updates`) until the live
    /// value catches up and matches, exactly the same self-deferring shape
    /// as `pending_culture_check` above -- confirmed necessary by tracing
    /// `7523809` line 344: Orange's live culture there is 64 (this turn's
    /// own War-over-Culture win, +22, not yet cascaded in), not BGO's stated
    /// 86, and only reaches 86 a few lines later once Purple's own blocked
    /// `EndTurn` (line 343) finally resolves and hands the turn to Orange.
    pending_culture_corrections: Vec<(Color, i32, i32)>,
    /// The resources twin of [`Replayer::pending_culture_corrections`] --
    /// same BGO admin-correction line shape, same self-deferring retry, but
    /// for `"GAME DATA UPDATED <Color> Bronze: <old> -> <new>"` clauses
    /// (`parse_game_data_updated_resources`'s own doc for why the stat is
    /// spelled "Bronze", not "resources"). `classify` filed the WHOLE line
    /// as `Bookkeeping` (see its own comment: "an admin correction BGO
    /// occasionally injects"), and until this field existed nothing ever
    /// applied a `Bronze` clause either -- confirmed the direct cause of a
    /// dominant `IllegalMove: WonderStep` sub-cluster (`docs/REPLAY.md`):
    /// game `7521364` line 22 reads `"GAME DATA UPDATED Orange Bronze: 2 ->
    /// 6"` one line before Orange's own `"builds 1 stage of Colossus ...
    /// spends 3 resources"` (line 23), which this binary rejected as illegal
    /// with `resources=2` -- exactly the un-applied correction's own `old`
    /// value, one short of the stage's cost of 3. Same story for `7523085`
    /// line 240 (`"Green Bronze: 4 -> 9"`, a +5 the reconstruction never
    /// saw) against its own WonderStep failure five lines later.
    pending_resource_corrections: Vec<(Color, i32, i32)>,
    /// The science twin of [`Replayer::pending_culture_corrections`] -- same
    /// BGO admin-correction line shape, same self-deferring retry, but for
    /// `"GAME DATA UPDATED <Color> science: <old> -> <new>"` clauses
    /// (`parse_game_data_updated_science`'s own doc for why this one exists:
    /// `classify` filed the WHOLE line as `Bookkeeping` and nothing ever
    /// applied a `science` clause until now -- confirmed the direct cause of
    /// the `IllegalMove: Develop` bucket (`docs/REPLAY.md`): game `7522617`
    /// line 105 reads `"GAME DATA UPDATED Green science: 7 -> 8; Green
    /// Bronze: 4 -> 5"` one line before Green's own `"discovers Monarchy ...
    /// loses 8 science"` (line 107), which this binary rejected as illegal
    /// with `science=7` -- exactly the un-applied correction's own `old`
    /// value, one short of Monarchy's science cost of 8). Science is a plain
    /// scalar like culture (no token-bank involvement), so the flush
    /// mirrors the culture one, not the resources one.
    pending_science_corrections: Vec<(Color, i32, i32)>,
    /// A structural "false skip" instrument -- see [`GameResult::
    /// politics_false_skips`]'s own doc for the full mechanism this counts.
    /// Incremented at most once per `plan.preparations` entry (tracked via
    /// [`Replayer::false_skip_flagged_prep`]), not once per
    /// `resolve_intervening` loop iteration, so a decision the loop revisits
    /// many times before making progress is not over-counted.
    politics_false_skips: u32,
    /// The `next_prep` index already counted toward `politics_false_skips`,
    /// if any -- prevents [`Replayer::resolve_intervening`]'s own retry loop
    /// (up to 200 iterations against the SAME unresolved decision) from
    /// inflating the count past one per genuine false skip.
    false_skip_flagged_prep: Option<usize>,
    /// See [`GameResult::politics_false_skips_unrecovered`] -- the TRUE
    /// damage signal, as opposed to `politics_false_skips`'s raw occurrence
    /// count. Every `politics_false_skips` detection now attempts an
    /// immediate recovery (`resolve_intervening`'s own doc); this only
    /// increments on the rare case that recovery itself fails.
    politics_false_skips_unrecovered: u32,
    /// `cardblame`'s own corruption cross-check, one entry per journal "End
    /// turn" line: does this reconstruction's own `economy::corruption(
    /// economy::blue_available(..))` (computed independently, NOT by
    /// letting `economy::end_of_turn` run and reading a mutated field back)
    /// agree with whether BGO's own journal line carries a `"CORRUPTION!"`
    /// marker? See [`CorruptionCheck`]'s own doc for why this is safe to
    /// compute BEFORE `try_apply(Move::EndTurn, ..)` runs.
    corruption_checks: Vec<CorruptionCheck>,
    /// `cardblame`'s widened-attribution hook (task 2026-08-14): every
    /// `Move` this replayer actually applied to `self.state`, in order --
    /// see [`Self::apply_move`]'s own doc for why this is the single choke
    /// point rather than something bolted onto `try_apply` alone. Read by
    /// [`game_ever_cards_in_play`] to build [`GameResult::
    /// ever_cards_in_play`], and exposed directly as [`GameResult::
    /// moves_applied`] for `cardblame`'s Move-keyed happenings cut.
    moves_applied: Vec<Move>,
    /// Every event card this replayer actually RESOLVED, in order -- see
    /// [`Self::apply_move`]'s own doc for why a resolved event is not the
    /// `CardId` argument of any `Move` and so cannot be recovered from
    /// [`Self::moves_applied`].
    events_resolved: Vec<CardId>,
    /// Every journal line's own [`ActionClass`] classification, in the
    /// order `apply_one` saw them -- pushed once per `apply_one` call (see
    /// that function's own first statement). `ActionClass` is BGO's own
    /// line-shape classification (`corpus::classify`), independent of
    /// `Move` -- several `ActionClass` variants (`WinAuction`, `WinWar`,
    /// `PlayEvent`, `Discard`, ...) are pure confirmation lines with no
    /// `Move` of their own (`apply_one`'s own `Ok(())` arms), so this is
    /// the only place those happenings are visible at all. Exposed as
    /// [`GameResult::line_classes`] for `cardblame`'s ActionClass-keyed
    /// happenings cut.
    line_classes: Vec<ActionClass>,
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

/// The real scoringEvent cards named on the journal's own `"End of game"`
/// line -- BGO prints this game's exact still-pending "Impact of ..." set
/// directly (`"End of game Check the journal to get the final impacts
/// effects :; Impact of X; Impact of Y; ...; WINNER IS ..."`), the one piece
/// of ground truth about the unrevealed tail of the event decks this
/// project was not yet using. Semicolon-separated; stops naturally at the
/// `"WINNER IS"`/`"; WINNER IS"` clause, which never starts with `"Impact
/// of"`. A card name not in `card_index` is skipped rather than panicking:
/// this line is read for every completed game, including any future corpus
/// entry whose event names this table does not yet cover, and a missed
/// grounding is strictly no worse than the pre-existing fictional pile this
/// replaces it with.
fn parse_real_final_events(text: &str, card_index: &HashMap<&'static str, CardId>) -> Vec<CardId> {
    // `dedup` on CardId, not on the clause string: two DIFFERENT card names
    // can never collide here (each maps to a distinct id), but the SAME name
    // can appear twice in one announcement. BGO logs the marker line
    // per-seat (one line per player's turn, identical text), and the
    // replay's top-of-loop `game_over` guard stops after the first copy --
    // which works only for journals whose FIRST copy is the one whose seat
    // triggered game end. A journal that logs the trigger seat's copy LAST
    // (7523253: Grey's copy precedes Orange's) reaches the "End of game"
    // marker with `game_over` still false and feeds this function a line
    // whose pending SET is already scored TWICE -- one card per logged copy.
    // The announced SET is a set, so keep one entry per card, in order.
    let mut out = Vec::new();
    for clause in text.split(';').map(str::trim).filter(|c| c.starts_with("Impact of")) {
        let Some(&card) = card_index.get(clause) else { continue };
        if !out.contains(&card) {
            out.push(card);
        }
    }
    out
}

/// Overwrite the still-pending event piles with exactly the real cards
/// [`parse_real_final_events`] found -- the fix for the "wrong final-event
/// SET" mechanism `docs/REPLAY.md`'s "Final scores" section already named:
/// `events::evaluate_final_events` reads `pending_final_events`
/// (`current_events` chained with `future_events`), and for cards never
/// revealed in the real game, `event_plan`'s own module doc already admits
/// those piles are "filled with unused cards of the right age and kind"
/// with "nothing ever validates them" -- correct for legality (deck size/
/// age profile), silently wrong for final scoring. Which pile a card ends
/// up in does not matter: `pending_final_events` chains both, unordered.
/// Any non-scoring card already in either pile (a Territory this
/// reconstruction still holds, irrelevant to `evaluate_final_events`'s own
/// `final_scoring_block`-filtered read) is dropped along with them -- game
/// over fires on the very next line and nothing else ever reads these piles
/// again.
/// One (card, seat) pair where BGO's own journal-stated §12.5.2 award
/// disagrees with this reconstruction's own `events::scoring_culture`
/// recompute -- see [`parse_final_event_journal_amounts`] and
/// `GameResult::final_event_award_divergences`'s own doc.
#[derive(Debug, Clone)]
pub struct FinalEventAwardDivergence {
    pub card: &'static str,
    pub seat: u8,
    pub journal_amount: i32,
    pub engine_amount: i32,
}

/// Ground truth for EVERY still-pending "Impact of ..." card's own per-seat
/// award, read directly off the journal's own announcement line for that
/// card (`"Impact of X <description>; Colour scores N culture; Colour2
/// loses M culture; ..."`) -- independent of [`parse_real_final_events`]'s
/// "End of game" SUMMARY line (which only names the pending SET, not the
/// amounts) and independent of grounding-order concerns entirely: whichever
/// journal line actually carries a card's award text is read wherever it
/// falls, so a game where `finish_game` fired off the wrong pile before
/// `ground_final_events` ran (`GAMEDROP.txt`'s "TruncatedAfterGameOver" causes
/// this, `replay_game_completes_a_journal_whose_duplicated_end_turn_pair_is_
/// logged_before_its_own_end_of_game_marker` test) still gets a correct
/// oracle reading, because the WRONG (fictional) cards this binary computed
/// never have their OWN "Impact of ..." line anywhere in a real journal
/// (checked: game 7522906's fictional Industry/Agriculture/Competition
/// award never appears in its own journal text at all) -- there is nothing
/// for those cards to be compared against, so they are silently absent from
/// the returned map rather than reported as false disagreements.
///
/// A card that fires mid-game via a "plays event" line AND is still
/// pending at game end is announced TWICE in its journal -- but ONLY the
/// end-of-game line is read here: `finish_game`'s `evaluate_final_events`
/// scores every still-pending card once per seat at game end, so the
/// journal's end-of-game re-award is the one that must be compared
/// against the engine's end-of-game award. The in-play line (whose first
/// clause is the player's own age-bank, not a payout) is skipped by the
/// `plays event` check in the loop body -- it was evaluated one or two
/// rounds early, so its amounts are not ground truth for any end-of-game
/// comparison. The per-seat OVERWRITE that remains is a no-op in real
/// journals (no card has two END-OF-GAME lines; the only other line
/// shape, "End of game ... Impact of X; ..." the summary marker, carries
/// no amounts and is dropped by the `awards.is_empty()` guard below),
/// kept as a safety net rather than a load-bearing rule.
fn parse_final_event_journal_amounts(
    journal: &[Line<'_>],
    card_index: &HashMap<&'static str, CardId>,
) -> HashMap<CardId, Vec<(u8, i32)>> {
    let mut out: HashMap<CardId, Vec<(u8, i32)>> = HashMap::new();
    // Only the 15 base-game `Special::FinalScoring` cards can ever open one
    // of these lines -- precomputed once so the per-line hot loop below is a
    // handful of `starts_with` checks, not a full `card_index` scan.
    let scoring_cards: Vec<(&'static str, CardId)> =
        card_index.iter().filter(|(name, _)| name.starts_with("Impact of ")).map(|(&n, &c)| (n, c)).collect();
    for line in journal {
        // IN-PLAY PAYOUT LINES ARE NOT END-OF-GAME GROUND TRUTH. BGO logs a
        // `<Colour> plays event <Colour> scores N culture; Current event:;
        // III / <Impact of X>; <description>; <seat> scores M culture; ...`
        // line the moment a scoring event is revealed MID-GAME (71 of the
        // 749-game corpus do this for Impact of Progress alone). The line's
        // first clause is the playing player's own age-bank ("scores 3
        // culture" -- the +3 of the Age-III card's level), NOT a payout
        // amount; the rest of the line is the card's MID-BOARD payout,
        // evaluated one or two rounds before game end, so it is ground
        // truth for NO end-of-game comparison. The oracle below compares
        // `final_event_awards` -- the game-END recompute -- so only the
        // end-of-game line's clauses may be read here. The end-of-game
        // line ALWAYS follows its in-play twin (checked 2026-08-16: zero
        // corpus games have an in-play line without an end-of-game line
        // for the same card), so skipping in-play lines loses nothing and
        // kills the phantom divergence the OVERWRITE rule below used to
        // produce: the in-play line was read first, then the end-of-game
        // line's clauses overwrote it per seat -- except the in-play line's
        // own first clause ("<Colour> scores 3 culture") was never
        // overwritten for its seat when that seat's end-of-game amount sat
        // in a LATER clause, leaving the stale mid-game value in the map.
        // Game `7521849` (2026-08-16): its end-of-game line states "Orange
        // scores 12 culture; Purple scores 12 culture" -- the journal's own
        // arithmetic error (Purple's board scores 14 under the card's own
        // wording; the engine's 14 is the final-score cross-check's value)
        // -- and the engine's per-seat awards now compare against exactly
        // that line.
        let first_clause = line.text.split(';').next().map(str::trim);
        let is_in_play = first_clause.is_some_and(|c| c.contains("plays event"));
        let Some(first_clause) = first_clause else { continue };
        if is_in_play {
            continue;
        }
        let Some(&(_, card)) = scoring_cards.iter().find(|(name, _)| first_clause.starts_with(name)) else {
            continue;
        };
        let mut awards = Vec::new();
        for clause in line.text.split(';').skip(1).map(str::trim) {
            let words: Vec<&str> = clause.split_whitespace().collect();
            let [colour_word, verb, amount_word, "culture"] = words[..] else { continue };
            let Some(colour) = crate::corpus::Color::parse(colour_word) else { continue };
            let Ok(amount) = amount_word.parse::<i32>() else { continue };
            let signed = match verb {
                "scores" => amount,
                "loses" => -amount,
                _ => continue,
            };
            awards.push((colour.seat(), signed));
        }
        if awards.is_empty() {
            continue;
        }
        // Overwrite, not extend: a later line for the same (card, seat) is
        // the end-of-game re-award and supersedes the earlier in-play one.
        let entry = out.entry(card).or_default();
        for (seat, signed) in awards {
            if let Some(slot) = entry.iter_mut().find(|(s, _)| *s == seat) {
                slot.1 = signed;
            } else {
                entry.push((seat, signed));
            }
        }
    }
    out
}

fn ground_final_events(state: &mut GameState, real_cards: &[CardId]) {
    state.current_events = crate::state::CardList::new();
    state.future_events = crate::state::CardList::new();
    for &card in real_cards {
        state.future_events.push(card);
    }
}

impl<'a> Replayer<'a> {
    // Grouping these into a config struct is a real fix but a larger,
    // cross-cutting refactor (every call site would need updating too) --
    // out of scope for this lint-gate pass, which must not change behaviour.
    // The argument list itself is stable and each parameter is unambiguous
    // at every call site.
    #[allow(clippy::too_many_arguments)]
    fn new(
        card_index: &'a HashMap<&'static str, CardId>,
        num_players: u8,
        plan: EventPlan,
        gain_produces: HashMap<u8, VecDeque<(bool, i32)>>,
        plunder_splits: HashMap<u8, VecDeque<(i16, i16)>>,
        infiltrates: HashMap<u8, VecDeque<bool>>,
        lose_pop_destroys: HashMap<u8, VecDeque<(usize, CardId)>>,
        raid_destroys: VecDeque<CardId>,
        lose_colonies: HashMap<u8, VecDeque<CardId>>,
        flip_wonders: HashMap<u8, VecDeque<CardId>>,
        future_military_needs: HashMap<u8, Vec<FutureNeed>>,
        colonize_sacrifices: VecDeque<ColonizeSacrifice>,
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
            military_hand_deficit: [0; 4],
            colonize_approximated: false,
            bid_ceilings_grounded: 0,
            hand_full_takes_overridden: 0,
            colonize_sacrifices,
            actions_consumed: 0,
            trailing_discards_lines: Vec::new(),
            duplicate_build_lines: Vec::new(),
            current_lineno: 0,
            gain_produces,
            plunder_splits,
            produces_grants: HashMap::new(),
            spends_grants: HashMap::new(),
            infiltrates,
            lose_pop_destroys,
            raid_destroys,
            lose_colonies,
            flip_wonders,
            war_tech_spoils: HashMap::new(),
            claimed_destroy_lines: std::collections::HashSet::new(),
            claimed_civil_life_lines: std::collections::HashSet::new(),
            discard_solver: DiscardSolver::new(future_military_needs),
            record_decisions: false,
            decisions: Vec::new(),
            discard_phase_oracle: HashMap::new(),
            discard_oracle_divergence: None,
            discard_oracle_checked: 0,
            discard_oracle_agreed: 0,
            military_hand_ledger: HashMap::new(),
            hand_ledger_verdict: None,
            last_action_class: None,
            culture_oracle_divergence: None,
            culture_oracle_checked: 0,
            culture_oracle_agreed: 0,
            pending_culture_check: None,
            science_oracle_divergence: None,
            science_oracle_checked: 0,
            science_oracle_agreed: 0,
            pending_science_check: None,
            resource_oracle_divergence: None,
            resource_oracle_checked: 0,
            resource_oracle_agreed: 0,
            pending_resource_check: None,
            pending_culture_corrections: Vec::new(),
            pending_resource_corrections: Vec::new(),
            pending_science_corrections: Vec::new(),
            politics_false_skips: 0,
            false_skip_flagged_prep: None,
            politics_false_skips_unrecovered: 0,
            corruption_checks: Vec::new(),
            moves_applied: Vec::new(),
            events_resolved: Vec::new(),
            line_classes: Vec::new(),
        }
    }

    /// The single choke point every `Move` this file actually applies to
    /// `self.state` goes through -- called both from `try_apply`'s own
    /// happy path and from every other direct `apply::apply(&mut
    /// self.state, ..)` site in this file (internal `Move::Choose` replies
    /// resolving a pending decision, `try_apply_take`'s hand-full bypass,
    /// ...), none of which go through `try_apply`'s legality check because
    /// the `Move` is already known-legal by construction at that point.
    /// Records `mv` into `Self::moves_applied` first -- purely additive
    /// bookkeeping; does not change what `apply::apply` does to
    /// `self.state`, and every caller's control flow is unchanged (same
    /// two statements it replaces, in the same order).
    ///
    /// Also records any card that LEFT `current_events` while this move was
    /// applied, into [`Self::events_resolved`]. `Move::PrepareEvent`'s own
    /// `CardId` argument is the card being pushed onto `future_events`, NOT
    /// the one `events::reveal_current_event` pops off `current_events` and
    /// resolves inside that same call -- two different cards from one move.
    /// Event cards never enter a tableau either, so without this the entire
    /// Age A military deck ("Development of X") and every other resolved
    /// event was invisible to all of `cardblame`'s attribution paths. The
    /// diff is taken here rather than reading the pop directly because
    /// `set_current_events` rebuilds the pile wholesale from the journal's
    /// own reveal order AFTER the move returns; this window sees only what
    /// `apply::apply` itself consumed.
    fn apply_move(&mut self, mv: Move) {
        self.moves_applied.push(mv);
        let before: Vec<CardId> = self.state.current_events.as_slice().to_vec();
        // A `Bid`/`BidPass` is the ONLY move that can settle an auction
        // (`interact::auction_move`'s own match: every other `Move` panics
        // against a live `Pending::Auction`), and settling one can run
        // `interact::colonize`'s own `colonize_auto` all the way to
        // completion with no branching step at all, inside this SAME call --
        // see `a_fully_forced_colonize_pops_its_own_queue_entry_...`'s doc.
        // When that happens, `colonize_sacrifices`' front entry (this file's
        // own replay bookkeeping, not engine state) must be popped HERE, at
        // the exact move that actually completed it -- not inferred later at
        // this territory's own confirmation line, which may already be
        // behind us, or may not be reached until several journal lines
        // later, if the auction itself did not resolve until AFTER that
        // line (a same-timestamp `WinAuction`/forced-`BidPass` collision;
        // `worker FINALINPUT`, game `7522866`'s own "Developed Territory" --
        // `analysis/worker_notes_2026-08-14/finalinput__FINALINPUT.txt`).
        // Comparing the TOTAL colony count across all seats before and after
        // is a purely structural signal that needs no territory-age or
        // actor-identity guessing: `interact::gain_colony` is the only place
        // that ever grows a `p.colonies`, and only one `Pending::Colonize`
        // can ever be open (and therefore completing) at a time, so a growth
        // of exactly one colony during this move can only be the queue's own
        // front entry.
        let colonies_before = matches!(mv, Move::Bid { .. } | Move::BidPass)
            .then(|| self.state.players.iter().map(|p| p.colonies.len()).sum::<usize>());
        apply::apply(&mut self.state, mv);
        if let Some(before_count) = colonies_before {
            let after_count: usize = self.state.players.iter().map(|p| p.colonies.len()).sum();
            if after_count > before_count {
                self.colonize_sacrifices.pop_front();
            }
        }
        let mut after: Vec<CardId> = self.state.current_events.as_slice().to_vec();
        for card in before {
            match after.iter().position(|&c| c == card) {
                Some(i) => {
                    after.swap_remove(i);
                }
                None => self.events_resolved.push(card),
            }
        }
    }

    /// `cardblame`'s corruption checkpoint -- called from the `EndTurn`
    /// dispatch arm at the SAME point as [`Self::check_discard_phase_
    /// oracle`] (right after `resolve_intervening`, right before `try_apply
    /// (Move::EndTurn, ..)`), which is deliberate: `economy::end_of_turn`'s
    /// own step 3b (`corr = corruption(blue_available(p))`) reads
    /// `p.food`/`p.resources`/`p.blue_total`/`p.wonder_steps` -- none of
    /// which step 3a (science/culture scoring, which runs first inside that
    /// SAME function) or anything earlier this turn touches -- so
    /// recomputing the identical two pure functions here, on `self.state`
    /// as it stands right now, reproduces EXACTLY the value `end_of_turn`
    /// is about to compute internally, without needing that mutating call
    /// to have run yet (it may not even run immediately -- a pending
    /// military discard can defer production past this line -- so reading a
    /// post-hoc field back would be unreliable; this is not). An uprising
    /// (`economy::uprising`) skips step 3 entirely, so it forces
    /// `engine_charges` to `false` regardless of the blue-token math, same
    /// as the real function.
    fn record_corruption_check(&mut self, actor: u8, line: &Line) {
        let p = &self.state.players[actor as usize];
        let engine_charges = if economy::uprising(&self.state, p) {
            false
        } else {
            economy::corruption(economy::blue_available(p)) > 0
        };
        let journal_charges = line.text.contains("CORRUPTION!");
        let cards_in_play = player_cards_in_play(p);
        self.corruption_checks.push(CorruptionCheck {
            lineno: line.lineno,
            actor: Color::from_seat(actor).map(Color::as_str).unwrap_or("?"),
            engine_charges,
            journal_charges,
            cards_in_play,
        });
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
            // A queued discard drained just above (the `DiscardMilitary`
            // branch below, most recent iteration) can itself finish the
            // LAST player's end of turn and run `game::finish_game` as a
            // side effect (`game::resume_end_turn`'s own doc). When that
            // happens there is nothing left to "intervene" on -- the game is
            // over -- but `decider` has already moved on to whoever
            // `game::advance_turn` set `state.current` to, so the ordinary
            // `decider == expected_actor` check below would never pass and
            // this would report a bogus stuck pending for what is actually a
            // clean finish. BGO logs the true final turn's own "End turn"
            // line (and its discard/"No Discard Phase" follow-up) TWICE, so
            // the caller sees this exact shape on the second, redundant copy
            // -- returning `Ok(())` here lets it fall through as a no-op
            // instead of a mismatch.
            if self.state.game_over {
                return Ok(());
            }
            let decider = self.state.decider();
            // Structural "false skip" instrument -- see [`GameResult::
            // politics_false_skips`]'s own doc for the mechanism this
            // detects: the journal's own solved plan says `decider` has a
            // real preparation waiting (its line has already been reached),
            // but this reconstruction's `state.phase` is no longer
            // `Politics` for them -- meaning `game::auto_skip_politics`
            // already fired against a `hand_military` this binary under-
            // tracked, closing the phase before `resolve_political_decision`
            // ever got a chance to claim it. Checked every loop iteration
            // (cheap: two field reads and an `Option` compare) but flagged
            // at most once per `next_prep` value via `false_skip_flagged_prep`.
            if let Some(prep) = self.plan.preparations.get(self.next_prep).copied() {
                if prep.actor == decider
                    && prep.lineno <= self.current_lineno
                    && self.state.phase != Phase::Politics
                    && self.false_skip_flagged_prep != Some(self.next_prep)
                    // `game::auto_skip_politics` only EVER fires with
                    // `state.pending` empty (`game.rs::start_turn`/
                    // `interact.rs`'s own `QueueItem::AutoSkipPolitics` arm
                    // both gate the call on it) -- but by the time THIS loop
                    // iteration reaches here, an unrelated pending choice for
                    // some OTHER decision can have opened since (this file
                    // drains many kinds of pending one iteration at a time).
                    // `decider` above is `state.decider()`, which reads
                    // `pending.top()`'s own player when pending is non-empty
                    // -- so `prep.actor == decider` can coincidentally match
                    // a decider who is mid an UNRELATED pending choice, not
                    // actually free to answer their political decision yet.
                    // Recovering right now would call `resolve_political_
                    // decision` -> `apply::apply(PrepareEvent)` while that
                    // pending is still open, which routes through `apply::
                    // apply`'s own pending-first branch instead of the real
                    // `h_prepare_event` handler -- confirmed on the corpus
                    // (`IllegalMove: PrepareEvent`, one game, before this
                    // guard). Skipping for now (not flagging/counting either
                    // -- this iteration hasn't actually failed anything) and
                    // re-checking next iteration, once whatever drained the
                    // pending above has run, is correct: this exact
                    // condition is re-evaluated fresh every iteration.
                    && self.state.pending.is_empty()
                {
                    self.politics_false_skips += 1;
                    self.false_skip_flagged_prep = Some(self.next_prep);
                    // THE FIX (this block used to be a detector only):
                    // `game::auto_skip_politics` already closed `decider`'s
                    // Politics phase against this reconstruction's own
                    // under-tracked `hand_military` -- see the comment above
                    // for the mechanism. `game::auto_skip_politics` itself is
                    // never called from this file (only from `game.rs`/
                    // `interact.rs`, both self-play's own code paths, whose
                    // hand tracking is exact and never trips this condition
                    // at all), so reopening the phase HERE cannot change
                    // self-play's behaviour by one bit -- it is a pure
                    // replay-side revert of a mistake replay-side under-
                    // tracking caused.
                    //
                    // Reopened AND claimed right here, synchronously, rather
                    // than just reopening `phase` and hoping the ordinary
                    // `own_politics_decision`/`claimable_preparation` path a
                    // few lines below happens to reach it before something
                    // else changes `decider` first: `resolve_political_
                    // decision` is the SAME call any on-time preparation
                    // goes through (it grounds the missing card into
                    // `hand_military` itself, `ground_bid_ceiling`'s own
                    // "pop one card of unknown provenance" convention -- not
                    // a parallel mechanism), and `?` here means a genuine
                    // recovery failure (the grounding itself hitting a real
                    // `IllegalMove`/`EventPlanInfeasible`) surfaces as a
                    // loud, specific `Mismatch` for this game, exactly like
                    // every other decision this file cannot resolve -- never
                    // silently swallowed. Measured clean across the full
                    // corpus: zero games hit this `?` (see `docs/REPLAY.md`).
                    // Counted separately into `politics_false_skips_
                    // unrecovered` (the TRUE damage signal -- see that
                    // field's own doc) before propagating, so a game that
                    // dies here still reports how far it got.
                    self.state.phase = Phase::Politics;
                    self.state.players[decider as usize].politics_done = false;
                    if let Err(kind) = self.resolve_political_decision(decider) {
                        self.politics_false_skips_unrecovered += 1;
                        return Err(kind);
                    }
                    continue;
                }
            }
            if crate::debugflags::replay_debug_all() {
                eprintln!(
                    "DEBUG resolve_intervening loop: lineno={} decider={decider} expected_actor={expected_actor} upcoming={upcoming:?} pending_top={:?}",
                    self.current_lineno,
                    self.state.pending.top()
                );
            }
            if let Some(Pending::Choice(c)) = self.state.pending.top().cloned() {
                match c.kind {
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
                        ChoiceKind::GainBlock => {
                        let picked = self
                            .gain_produces
                            .get_mut(&decider)
                            .and_then(|q| q.pop_front());
                        let n = match picked {
                            Some(picked) => c
                                .options
                                .as_slice()
                                .iter()
                                .position(|o| match o {
                                    ChoiceOption::Gain(g) => {
                                        (picked.0 && g.resources as i32 == picked.1 && picked.1 != 0)
                                            || (!picked.0 && g.food as i32 == picked.1 && picked.1 != 0)
                                    }
                                    ChoiceOption::Card(_) | ChoiceOption::Slot(_) | ChoiceOption::Move(_) | ChoiceOption::Word(_) => false,
                                })
                                .ok_or_else(|| {
                                    MismatchKind::ParserGap(format!(
                                        "GainBlock options {:?} do not offer the journal-observed {} {}",
                                        c.options.as_slice(),
                                        picked.1,
                                        if picked.0 { "resources" } else { "food" }
                                    ))
                                })?,
                            None => {
                                // No journal-observed "produces" line: guess
                                // the first Gain option.
                                c.options
                                    .as_slice()
                                    .iter()
                                    .position(|o| matches!(o, ChoiceOption::Gain(_)))
                                    .ok_or_else(|| {
                                        MismatchKind::StuckPending(
                                            "GainBlock choice has no Gain option to guess".into(),
                                        )
                                    })?
                            }
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // A `Pending::Choice(DestroyOwn)` (event §5.3 `weakestPlayer`
                    // `destroyOwnBuilding` -- today the ONLY base-game cards
                    // that print it are `Border Conflict` / `Uncertain Borders`)
                    // is opened for the WEAKEST player. BGO logs the destroyed
                    // building as an ordinary `"<Color> destroys <Card>"` line
                    // (`ActionClass::Destroy`), the SAME shape as a voluntary
                    // destroy, so `apply_one`'s `Destroy | Disband` arm
                    // ALREADY resolves it as a `Choose` when the choice is
                    // still open (see that arm's `ChoiceKind::DestroyOwn |
                    // ChoiceKind::LosePop` check). This arm exists only to
                    // keep the match exhaustive (the `DestroyOwn` kind used to
                    // fall through to the no-op group below, which was correct
                    // for the same-line case but is now handled here to be
                    // explicit).
                    ChoiceKind::DestroyOwn => {}
                    // A `Pending::Choice(PlunderSplit)` (Aggression: Plunder's
                    // "your rival loses a total of up to N resource and/or food
                    // (your choice)") is opened for the ATTACKER only, but --
                    // exactly like `GainBlock` above, and for the same reason
                    // (`docs/REPLAY.md`'s six-pending-kind pass) -- it blocks
                    // ANY further action by that player, including an unrelated
                    // one several lines later, not just their own reply to the
                    // aggression. Drained unconditionally here from
                    // `plunder_splits` (`prescan_plunder_splits`'s doc explains
                    // why a popped entry is VALIDATED against this choice's own
                    // options, and skipped rather than trusted by position, if
                    // it doesn't match -- a single-option Plunder split never
                    // opens a pending at all, so the FIFO can contain entries
                    // this function is never asked to consume).
                        ChoiceKind::PlunderSplit { .. } => {
                        let q = self.plunder_splits.entry(decider).or_default();
                        let n = loop {
                            let Some(&(food, resources)) = q.front() else {
                                // No journal-observed Plunder resolution: guess
                                // the first Gain option.
                                break c
                                    .options
                                    .as_slice()
                                    .iter()
                                    .position(|o| matches!(o, ChoiceOption::Gain(_)))
                                    .ok_or_else(|| {
                                        MismatchKind::StuckPending(
                                            "PlunderSplit choice has no Gain option to guess".into(),
                                        )
                                    })?;
                            };
                            if let Some(idx) = c.options.as_slice().iter().position(|o| {
                                matches!(o, ChoiceOption::Gain(g) if g.food == food && g.resources == resources)
                            }) {
                                q.pop_front();
                                break idx;
                            }
                            // Belongs to an earlier, single-option split this
                            // same player's Plunder already auto-resolved
                            // silently (see the field's own doc) -- not this
                            // choice's answer, skip past it and keep looking.
                            q.pop_front();
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // A `Pending::Choice(FoodOrResSplit)` (Raiders'/Foray's §5.3
                    // `foodAndOrResources` gain/lose block, FOODFIX) is SELF-
                    // directed rather than attacker-directed -- opened for the
                    // TARGETED player, who may not be `decider` (exactly
                    // `LosePop` above: `resolve_count_targets` can target a
                    // player other than whoever is nominally up next), so this
                    // reads `c.player`, not `decider`, like that arm does.
                    // Drained from `produces_grants` (gain, Foray) or
                    // `spends_grants` (loss, Raiders) -- the SAME per-seat FIFOs
                    // a prior pass built to POST-HOC patch `p.food`/`p.resources`
                    // after `events::food_or_resources`'s old fixed-formula call
                    // (`prescan_produces_grants`/`prescan_spends_grants`'s own
                    // docs); now repointed at draining this real choice instead.
                    // UNLIKE `PlunderSplit`'s FIFO above, a non-matching entry
                    // here is NOT popped/discarded while scanning -- these two
                    // FIFOs are filled by a broad, non-event-aware text-shape
                    // scan (e.g. a `GainBlock` choice's own "produces" line can
                    // sit in the same per-seat queue, see those prescans' own
                    // docs), so an entry this exact draw does not want may still
                    // belong to a LATER, real consumer further down the game;
                    // this scans by POSITION and removes only the match, the
                    // same non-destructive lookup the old post-hoc correction
                    // this replaced already used (`Vec::remove`, not
                    // `pop_front`) -- see `foray_skips_a_foreign_non_matching_
                    // entry_queued_ahead_of_its_own_real_split`'s own regression
                    // test for why popping the miss would be a real corpus bug.
                    ChoiceKind::FoodOrResSplit { lose } => {
                        let q = if lose {
                            self.spends_grants.entry(c.player).or_default()
                        } else {
                            self.produces_grants.entry(c.player).or_default()
                        };
                        let Some(pos) = q.iter().position(|&(jf, jr)| {
                            c.options.as_slice().iter().any(|o| matches!(o, ChoiceOption::Gain(g) if g.food == jf && g.resources == jr))
                        }) else {
                            // No journal-observed line: guess the first Gain option.
                            let idx = c
                                .options
                                .as_slice()
                                .iter()
                                .position(|o| matches!(o, ChoiceOption::Gain(_)))
                                .ok_or_else(|| {
                                    MismatchKind::StuckPending(
                                        "FoodOrResSplit choice has no Gain option to guess".into(),
                                    )
                                })?;
                            self.apply_move(Move::Choose { n: idx as u8 });
                            continue;
                        };
                        let (jf, jr) = q.remove(pos).expect("position just found by iter()");
                        let idx = c
                            .options
                            .as_slice()
                            .iter()
                            .position(|o| matches!(o, ChoiceOption::Gain(g) if g.food == jf && g.resources == jr))
                            .expect("this exact (food, resources) pair matched the `position` search above");
                        self.apply_move(Move::Choose { n: idx as u8 });
                        continue;
                    }
                    // A `Pending::Choice(Infiltrate)` (Aggression: Infiltrate's
                    // "remove your rival's leader or incomplete wonder from
                    // play" -- offered only when the victim has BOTH, otherwise
                    // `push_choice`'s own auto-resolve-if-len-1 rule settles it
                    // with no `Pending` at all) is opened for the ATTACKER only,
                    // but exactly like `PlunderSplit` above -- and for the same
                    // reason -- it blocks ANY further action by that player.
                    // Flagged mid-pass by a concurrent worker (the Take bucket)
                    // as the sixth kind sharing this same gap. Drained
                    // unconditionally from `infiltrates`, validated against this
                    // choice's own options and skipped (not trusted by
                    // position) exactly like `PlunderSplit`'s own FIFO -- an
                    // auto-resolved single-option Infiltrate (victim has only a
                    // leader OR only a wonder) prints the IDENTICAL resolving
                    // text shape with no real choice behind it, so the FIFO can
                    // carry entries this function is never asked to consume.
                        ChoiceKind::Infiltrate { .. } => {
                        let q = self.infiltrates.entry(decider).or_default();
                        let n = loop {
                            let Some(&is_wonder) = q.front() else {
                                return Err(MismatchKind::StuckPending(format!(
                                    "Infiltrate choice open for player {decider} but no journal-observed \
                                     Infiltrate resolution left to resolve it with"
                                )));
                            };
                            let want = if is_wonder { Keyword::Wonder } else { Keyword::Leader };
                            if let Some(idx) = c.options.as_slice().iter().position(|o| matches!(o, ChoiceOption::Word(w) if *w == want)) {
                                q.pop_front();
                                break idx;
                            }
                            // Belongs to an earlier, single-option Infiltrate
                            // this same player already auto-resolved silently
                            // (see the field's own doc) -- not this choice's
                            // answer, skip past it and keep looking.
                            q.pop_front();
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
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
                        ChoiceKind::FreeBuild => {
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
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // A `Pending::Choice(TakeRow { .. })` (International
                    // Agreement: spend up to `budget` civil actions taking row
                    // cards one at a time, `interact::offer_take_row`) is BGO's
                    // journal-logged the exact SAME way an ordinary `Move::Take`
                    // is -- `"<Color> takes <Card> in hand <Color> uses N civil
                    // action"` -- and, like `FreeBuild` just above, a human
                    // DECLINING it (picking `Word(Stop)`) leaves no journal
                    // trace at all (the same "no journal trace for a silent
                    // decline" precedent already established for Politics-phase
                    // passes and `FreeBuild`). If the upcoming line is
                    // `expected_actor`'s own take of a card that's still among
                    // this choice's own `Slot` options, stop here and let
                    // `apply_one`'s `TakeCard` arm (below) resolve it as a
                    // `Choose`, not a bare `Take` (illegal while this pending
                    // sits open); otherwise assume `Stop` and keep draining --
                    // covers the `decider != expected_actor` `StuckPending`
                    // shape too, exactly like `FreeBuild`'s own fallback (see
                    // `docs/REPLAY.md`'s six-pending-kind pass).
                        ChoiceKind::TakeRow { .. } => {
                        let matches_upcoming = c.player == expected_actor
                            && upcoming.0 == ActionClass::TakeCard
                            && upcoming.1.is_some_and(|want| {
                                c.options.as_slice().iter().any(|o| match o {
                                    // Either this slot is ALREADY grounded to
                                    // `want` by identity (the coincidence
                                    // check `ground_row_slot`'s own first
                                    // branch trusts for the same reason), OR
                                    // it is still ungrounded -- i.e. still
                                    // holding `new_game`'s fictional filler,
                                    // free to be forced into `want` the same
                                    // "believe the journal" way every OTHER
                                    // take on this file ultimately resolves
                                    // ambiguity (`ground_row_slot`'s own doc:
                                    // "placing the card in whichever slot
                                    // happens to be cheapest anyway... is the
                                    // honest report", just applied one level
                                    // up here). Required for a card this
                                    // binary has never grounded ANYWHERE yet
                                    // -- routinely true for a WONDER taken
                                    // via International Agreement, which
                                    // `card_row` never held before this exact
                                    // pick (no earlier "takes"/"builds" line
                                    // to have grounded it against) -- without
                                    // this, `matches_upcoming` was always
                                    // false for such a card, silently
                                    // auto-declining the whole choice via
                                    // `Stop` before `apply_one`'s own
                                    // `TakeCard` arm (which DOES know how to
                                    // ground an ungrounded slot) ever ran.
                                    // Corpus-confirmed regression: games
                                    // `7522957`/`7523114` (`Reserves`/`First
                                    // Space Flight`), caught by this bucket's
                                    // own regression bar.
                                    ChoiceOption::Slot(slot) => {
                                        self.state.card_row[*slot as usize] == want || !self.row_grounded[*slot as usize]
                                    }
                                    ChoiceOption::Card(_) | ChoiceOption::Move(_) | ChoiceOption::Gain(_) | ChoiceOption::Word(_) => false,
                                })
                            });
                        if matches_upcoming {
                            return Ok(());
                        }
                        let n = c
                            .options
                            .as_slice()
                            .iter()
                            .position(|o| matches!(o, ChoiceOption::Word(Keyword::Stop)))
                            .ok_or_else(|| MismatchKind::StuckPending("TakeRow choice has no Stop option".into()))?;
                        self.apply_move(Move::Choose { n: n as u8 });
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
                        ChoiceKind::DiscardMilitary => {
                        let matches_upcoming = c.player == expected_actor && upcoming.0 == ActionClass::Discard;
                        if matches_upcoming {
                            return Ok(());
                        }
                        self.resolve_one_discard_choice(&c);
                        continue;
                    }
                    // A `Pending::Choice(LosePop)` (a forced "lose 1 population"
                    // with no free worker to absorb it, `interact::run_item`'s
                    // `QueueItem::LosePop` arm) is resolved by `apply_one`'s
                    // `Destroy | Disband` arm ALREADY (mirroring `DestroyOwn`)
                    // when it is `expected_actor`'s own pending and the upcoming
                    // line IS their own `"destroys"` OR `"disbands"` line --
                    // BGO renders the SAME LosePop resolution as `"destroys"`
                    // when the surrendered worker-holder is a civil card and
                    // `"disbands"` when it is a military unit (the choice's own
                    // options mix both kinds, `worker_holding_options`) -- same
                    // `matches_upcoming` shape as `DiscardMilitary` just above,
                    // deferred there rather than duplicated here. Before this
                    // fix `matches_upcoming` only recognised `Destroy`, so every
                    // `"disbands"` resolution missed the fast path here and fell
                    // through to the `lose_pop_destroys` FIFO below, which (same
                    // bug, `prescan_lose_pop_destroys`) never indexed `Disband`
                    // lines either -- the pending then either errored as "no
                    // journal-observed destroy line" or, worse, silently stole
                    // an unrelated LATER real `Destroy` line sharing the same
                    // `CardId` (validated by identity only, not position),
                    // leaving THIS line's own resolution orphaned and `state.
                    // current` advanced past it -- the single largest confirmed
                    // cause of the `disbands`-shaped members of the `StuckPending:
                    // decider != expected actor ..., phase Actions, no pending`
                    // bucket (`docs/REPLAY.md`, traced on real games `7522649`,
                    // `7523045`, `7521377`).
                    //
                    // The gap this closes: the event that forces the loss can
                    // fire as a SIDE EFFECT of resolving a DIFFERENT player's
                    // political decision (`resolve_political_decision`, a few
                    // lines below, reached via the `None`/politics branch at the
                    // bottom of this loop when this function is catching up
                    // through outstanding political turns before `expected_
                    // actor`'s own) -- e.g. an event like Refugees/Pestilence
                    // that penalises "the weakest civilization", which need not
                    // be whoever is currently deciding anything. That leaves a
                    // LosePop pending open for `c.player`, genuinely unrelated
                    // to `expected_actor`'s own upcoming line, with `c.player`'s
                    // own resolving `"destroys"` line sitting SOMEWHERE ELSE in
                    // the journal (found on real games `7521344` -- opened
                    // resolving player 3's own political reveal while player 1
                    // was up next for an unrelated `Destroy`; that player's own
                    // resolution, `"Grey destroys Religion"`, doesn't appear
                    // until several lines later). Drained here from `lose_pop_
                    // destroys` (`prescan_lose_pop_destroys`'s doc explains why
                    // a popped entry is validated against the live choice's own
                    // options and skipped, not trusted by position, exactly like
                    // `PlunderSplit` -- an entry that doesn't match belongs to
                    // that same player's own unrelated voluntary `Destroy`/
                    // `DestroyOwn` choice, left alone for its own normal
                    // in-order processing). The claimed line's OWN index is
                    // recorded in `claimed_destroy_lines` so the main loop does
                    // not translate it a second time once its own pointer
                    // reaches it (see that field's doc) -- unlike `PlunderSplit`
                    // (`Bookkeeping`, always skipped by the main loop already) a
                    // `"destroys"` line is an ordinary action line the main loop
                    // would otherwise process again, double-applying the same
                    // destroy.
                        ChoiceKind::LosePop => {
                        let matches_upcoming =
                            c.player == expected_actor && matches!(upcoming.0, ActionClass::Destroy | ActionClass::Disband);
                        if matches_upcoming {
                            return Ok(());
                        }
                        let q = self.lose_pop_destroys.entry(c.player).or_default();
                        let n = loop {
                            let Some(&(line_idx, card)) = q.front() else {
                                return Err(MismatchKind::StuckPending(format!(
                                    "LosePop choice open for player {} but no journal-observed destroy line \
                                     left to resolve it with",
                                    c.player
                                )));
                            };
                            if let Some(idx) =
                                c.options.as_slice().iter().position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                            {
                                q.pop_front();
                                self.claimed_destroy_lines.insert(line_idx);
                                break idx;
                            }
                            // Belongs to this same player's own unrelated,
                            // separately-resolved voluntary destroy (see this
                            // block's own doc) -- not this choice's answer, skip
                            // past it and keep looking.
                            q.pop_front();
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }

                    // A `Pending::Choice(Raid)` (Aggression: Raid's "pick
                    // the urban building(s) to destroy" AND the Terrorism
                    // event's identical-shaped forced destruction) is
                    // opened for the ATTACKER (Raid) or has no natural
                    // "actor" at all (Terrorism, `corpus::classify`'s own
                    // `"Terrorists destroy a <Color> <Building>"` case,
                    // still `Bookkeeping` -- the destroyed card is right
                    // there, just discarded before this fix). Both print
                    // their resolution on a line this function never
                    // otherwise reads, so a GLOBAL (not per-player --
                    // Terrorism's own line never names an attacker)
                    // `VecDeque<CardId>` prescan (`prescan_raid_destroys`)
                    // feeds this exactly like `PlunderSplit`'s FIFO: popped
                    // in journal order, validated against the live choice's
                    // own options.
                    //
                    // A real Raid card's bracket is "up to N", genuinely
                    // declinable per bracket (`interact.rs`'s `QueueItem::
                    // Raid` doc), and now carries an explicit `Keyword::Stop`
                    // option for exactly that -- Terrorism (mandatory, no
                    // loot) never gets one. So: if the queue's front entry
                    // names a card that's actually among THIS choice's own
                    // options, that is unambiguously this bracket's answer,
                    // pop and take it; otherwise, if `Stop` is offered, the
                    // human declined this bracket -- take `Stop` and leave
                    // the queue untouched (its front entry, if any, belongs
                    // to a LATER Raid/Terrorism event). Only when `Stop`
                    // is NOT offered (Terrorism) does a front-entry mismatch
                    // fall back to the older "belongs to an earlier single-
                    // option Raid/Terrorism this same game already auto-
                    // resolved silently" skip-and-keep-looking (the risk
                    // `PlunderSplit`/`Infiltrate` already confirmed real --
                    // `push_choice`'s own auto-resolve-if-len-1 rule,
                    // `docs/REPLAY.md`'s six-pending-kind pass).
                    ChoiceKind::Raid { .. } => {
                        let stop_idx =
                            c.options.as_slice().iter().position(|o| matches!(o, ChoiceOption::Word(Keyword::Stop)));
                        let matches_options = |card: CardId| {
                            c.options.as_slice().iter().any(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                        };
                        // Discard any LEADING entries that don't match this
                        // choice's own options at all -- orphaned answers to
                        // an earlier, already auto-resolved single-candidate
                        // Raid/Terrorism (see this arm's own doc) -- same
                        // cleanup the old front-only loop already did.
                        while let Some(&card) = self.raid_destroys.front() {
                            if matches_options(card) {
                                break;
                            }
                            self.raid_destroys.pop_front();
                        }
                        // ENGINE BUG this closes (found chasing the
                        // `IllegalMove: Build` bucket's `workers_free == 0`
                        // sub-cluster, 2026-08-14 session, game `7522513`
                        // round 13): a real Raid II/III has TWO or THREE
                        // brackets with NESTED, not independent, eligibility
                        // (`interact.rs`'s `QueueItem::Raid`: `id.level() <=
                        // max_lv`, RULES_SPEC.md's "one of level 0-1 AND one
                        // of level 0-2") -- a low-level casualty can satisfy
                        // EITHER bracket while a high-level one can only ever
                        // satisfy the WIDE one. The journal's own `"Raid
                        // casualties ..."` line lists them by CARD NAME, not
                        // bracket-resolution order (confirmed: `7522513`
                        // prints "1 Alchemy; 1 Organized Religion", plain
                        // alphabetical order, and Organized Religion, level
                        // II, is not even a legal option for the level-0-1
                        // bracket) -- so taking whichever matching FIFO entry
                        // came first, as this code used to, can hand the
                        // WIDE bracket a casualty that the NARROW bracket
                        // could ALSO have taken, stranding the high-level one
                        // with nowhere left to go and wrongly declining it
                        // (`Stop`), permanently under-counting the victim's
                        // `workers_free` by one destroyed building's worth.
                        // Fix: among the CONTIGUOUS run of entries that DO
                        // match this bracket's own options (this bracket's
                        // own casualty group -- a non-match ends the run,
                        // the same boundary the leading-discard loop above
                        // already trusts), prefer the HIGHEST-level one --
                        // it has the fewest alternative homes -- leaving
                        // lower-level, more flexible entries in the FIFO for
                        // whichever bracket opens next.
                        let mut best: Option<(usize, CardId)> = None;
                        for (i, &card) in self.raid_destroys.iter().enumerate() {
                            if !matches_options(card) {
                                break;
                            }
                            if best.is_none_or(|(_, b)| card.level() > b.level()) {
                                best = Some((i, card));
                            }
                        }
                        let n = if let Some((fifo_idx, card)) = best {
                            self.raid_destroys.remove(fifo_idx);
                            c.options
                                .as_slice()
                                .iter()
                                .position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                                .expect("just matched by matches_options above")
                        } else if let Some(idx) = stop_idx {
                            idx
                        } else {
                            // No Stop option and no journal-observed destroy:
                            // guess the first Card option.
                            c.options
                                .as_slice()
                                .iter()
                                .position(|o| matches!(o, ChoiceOption::Card(_)))
                                .ok_or_else(|| {
                                    MismatchKind::StuckPending(
                                        "Raid choice has no Card option to guess".into(),
                                    )
                                })?
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // A `Pending::Choice(LoseColony)` (Independence
                    // Declaration: "the weakest civilization loses 1 colony,
                    // the player chooses which one") is opened for the
                    // victim only when they hold MORE THAN ONE colony --
                    // `push_choice`'s own auto-resolve-if-len-1 rule settles
                    // a single-colony victim with no `Pending` at all,
                    // printed inline on the SAME `"plays event"` line as
                    // `"<Territory> declares its independence from
                    // <Color>"` (glued in by `resolve_political_decision`/
                    // `PrepareEvent` machinery -- the shape the earlier
                    // handoff worried was unrecoverable). The REAL,
                    // multi-colony choice resolves on its own SEPARATE
                    // later line instead, in a different, unambiguous shape
                    // this function never previously read: `"<Color> loses
                    // <Territory family> (<Age numeral>)"` -- e.g. `"Purple
                    // loses Historic Territory (I)"` -- confirmed (`grep`
                    // over the full corpus) to always be its own clean,
                    // single-clause line with no trailing `;` continuation,
                    // and never to collide with the auto-resolved shape
                    // (which never appears on its own line at all). Drained
                    // here from a per-actor `lose_colonies` FIFO
                    // (`prescan_lose_colonies`), validated against the live
                    // choice's own options and skipped exactly like `Raid`'s
                    // FIFO above, for the same "an earlier auto-resolved
                    // single-colony case could otherwise misalign the
                    // queue" reason.
                    ChoiceKind::LoseColony => {
                        let q = self.lose_colonies.entry(decider).or_default();
                        let n = loop {
                            let Some(&territory) = q.front() else {
                                return Err(MismatchKind::StuckPending(format!(
                                    "LoseColony choice open for player {decider} but no journal-observed \
                                     \"loses <Territory>\" line left to resolve it with"
                                )));
                            };
                            if let Some(idx) =
                                c.options.as_slice().iter().position(|o| matches!(o, ChoiceOption::Card(id) if *id == territory))
                            {
                                q.pop_front();
                                break idx;
                            }
                            q.pop_front();
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // A `Pending::Choice(FlipWonder)` (Ravages of Time:
                    // "each player chooses one of their completed Age A/I
                    // wonders and turns it face down") is opened for a
                    // player only when they hold MORE THAN ONE qualifying
                    // wonder -- the single-candidate case again auto-
                    // resolves with no `Pending`, printed inline on the
                    // triggering `"plays event"` line itself (`"The <Wonder>
                    // crumble(s)"`, glued on next to `"<Color> must choose a
                    // wonder to ravage"` for whoever still owes a real
                    // choice). The REAL choice's own resolution is a
                    // SEPARATE later line with a DIFFERENT leading phrase
                    // this function never previously read: `"Ravages of
                    // Time <Wonder> crumble(s)"` -- no leading colour in the
                    // text at all, `Line::color` (column 2) is the only
                    // place the actor is, the same "no leading colour"
                    // shape `ColumbusColonize` already established a
                    // precedent for. Confirmed (`grep` over the full
                    // corpus) this standalone shape never carries a
                    // trailing `;` continuation and never collides with the
                    // auto-resolved inline shape (which is never its own
                    // line). Drained here from a per-actor `flip_wonders`
                    // FIFO (`prescan_flip_wonders`), validated against the
                    // live choice's own options and skipped exactly like
                    // `Raid`/`LoseColony` above.
                    ChoiceKind::FlipWonder => {
                        let q = self.flip_wonders.entry(decider).or_default();
                        let n = loop {
                            let Some(&wonder) = q.front() else {
                                return Err(MismatchKind::StuckPending(format!(
                                    "FlipWonder choice open for player {decider} but no journal-observed \
                                     \"Ravages of Time\" line left to resolve it with"
                                )));
                            };
                            if let Some(idx) =
                                c.options.as_slice().iter().position(|o| matches!(o, ChoiceOption::Card(id) if *id == wonder))
                            {
                                q.pop_front();
                                break idx;
                            }
                            q.pop_front();
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // A `Pending::Choice(WarTech)` (War over Technology's
                    // "science, or steal a blue special technology instead,
                    // repeat until the advantage runs out" -- FAQ p.8's
                    // "some or all" mixing) is opened for the VICTOR only,
                    // and only when the loser actually holds something
                    // stealable (`interact::offer_war_tech`'s own `opts.
                    // is_empty()` short-circuit means an unstealable war
                    // never creates a `Pending` at all, so this arm is only
                    // ever reached with a real decision to resolve). Each
                    // pick resolves to its OWN journal line (`"takes spoils
                    // of war ... steals <Card> ..."` or `"... gets N science
                    // ..."`) rather than one line per war, because `offer_
                    // war_tech` re-offers with the advantage reduced after
                    // every steal -- see `Replayer::war_tech_spoils`'s doc.
                    // Drained here in journal order, no validate-and-skip
                    // loop needed (unlike `Raid`/`LoseColony`/`FlipWonder`
                    // above): `prescan_war_tech_spoils`'s own doc explains
                    // why an orphaned earlier entry can't happen for this
                    // particular choice.
                    ChoiceKind::WarTech { .. } => {
                        let picked = self.war_tech_spoils.entry(decider).or_default().pop_front().ok_or_else(|| {
                            MismatchKind::StuckPending(format!(
                                "WarTech choice open for player {decider} but no journal-observed \
                                 \"takes spoils of war\" line left to resolve it with"
                            ))
                        })?;
                        let n = match picked {
                            WarSpoil::Steal(card) => c
                                .options
                                .as_slice()
                                .iter()
                                .position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                                .ok_or_else(|| {
                                    MismatchKind::ParserGap(format!(
                                        "WarTech options {:?} do not offer the journal-observed steal of {card:?}",
                                        c.options.as_slice()
                                    ))
                                })?,
                            WarSpoil::Science => crate::interact::WAR_TECH_SCIENCE_IDX as usize,
                        };
                        self.apply_move(Move::Choose { n: n as u8 });
                        continue;
                    }
                    // Every other `ChoiceKind` is left alone HERE, on
                    // purpose -- listed individually rather than behind a
                    // `_` wildcard, so a future `ChoiceKind` variant fails
                    // this match at compile time instead of silently
                    // inheriting whatever a catch-all happened to do (the
                    // exact shape that let all seven kinds this whole pass
                    // is working through go quietly unresolved for as long
                    // as they did, `docs/REPLAY.md`'s six/seven-pending-kind
                    // passes). `FreeCivil`/`FoodOrRes`/`DestroyOwn` are the
                    // player's OWN voluntary in-turn choice and fall through
                    // to the `decider == expected_actor` shortcut a few
                    // lines below, exactly like `DestroyOwn` always has;
                    // `Annex` has not been needed by any corpus game sampled
                    // so far; `PactOffer` is handled by the bottom match's
                    // own explicit arm (auto-`Refuse`) for the `decider !=
                    // expected_actor` case. Falling through here for all
                    // five is IDENTICAL to this match's predecessor (a chain
                    // of `if` statements that simply didn't mention them) --
                    // no behaviour changed, only exhaustiveness was added.
                    ChoiceKind::FreeCivil { .. }
                    | ChoiceKind::FoodOrRes { .. }
                    | ChoiceKind::PactOffer { .. } => {}
                    // A `Pending::Choice(Annex)` (Aggression: Annex's "take a
                    // rival's colony: gain its permanent effects", RULES_SPEC
                    // §5.5) is opened for the ATTACKER only (`interact.rs`'s
                    // `QueueItem::Annex`), but -- exactly like `TakeRow` above
                    // and `FreeBuild` before it -- BGO logs the colony the
                    // attacker actually takes as an ORDINARY `"takes <Card> in
                    // hand"` line (`ActionClass::TakeCard`), the SAME text
                    // shape as a bare `Move::Take`, with no distinct annex
                    // verb. If that take is the ATTACKER'S OWN upcoming line
                    // and the taken card is one of this choice's own options,
                    // stop here and let `apply_one`'s `TakeCard` arm resolve
                    // it as a `Choose` (a bare `Take` is illegal while this
                    // pending sits open). This closes the
                    // `IllegalMove: Take` / `StuckPending: pending choice
                    // Annex` sub-bucket first hit on real game `7522200`
                    // (Purple's `"plays Annex against Orange"` at round 10
                    // left the choice open, and Orange's next ordinary take
                    // was then rejected because the pending had never been
                    // drained). Unlike `TakeRow`/`FreeBuild`, Annex offers NO
                    // silent-decline `Stop`/`Skip` option (its options are
                    // the victim's colonies, `interact.rs`'s
                    // `QueueItem::Annex` arm), so an unmatched upcoming line
                    // is a genuine, unexplainable gap: fail loudly rather
                    // than steal a colony for a human.
                    ChoiceKind::Annex { .. } => {
                        let matches_upcoming = c.player == expected_actor
                            && upcoming.0 == ActionClass::TakeCard
                            && upcoming.1.is_some_and(|want| {
                                c.options.as_slice().iter().any(|o| matches!(o, ChoiceOption::Card(id) if *id == want))
                            });
                        if matches_upcoming {
                            return Ok(());
                        }
                        // The journal does not log which colony was taken.
                        // Guess: pick the first colony option.
                        let guess_n = c
                            .options
                            .as_slice()
                            .iter()
                            .position(|o| matches!(o, ChoiceOption::Card(_)))
                            .ok_or_else(|| {
                                MismatchKind::StuckPending(
                                    "Annex choice has no Card option to guess".into(),
                                )
                            })?;
                        self.apply_move(Move::Choose { n: guess_n as u8 });
                        continue;
                    }
                }
            }
            // A `Pending::Choice(DestroyOwn)` (event §5.3 `weakestPlayer`
            // `destroyOwnBuilding` -- today the ONLY base-game cards that
            // print it are the two weakest-civilization destroyers `Border
            // Conflict` / `Uncertain Borders`) is opened for the WEAKEST
            // player, who is NOT `expected_actor` when the event was
            // revealed on a DIFFERENT player's turn (`7521741` line 242:
            // Green reveals `Border Conflict`, Purple -- the weakest -- must
            // destroy, while Orange is up next). `interact::resolve_choice`
            // ALREADY validates the picked option against the live choice's
            // own options (a wrong pick is `IllegalMove`), so unlike the
            // `GainBlock`/`PlunderSplit` guesses this one needs no extra
            // safety: either the journal's answer is here or the state
            // diverged and the failure is loud. If the destroyed building
            // is this player's OWN upcoming line, `apply_one`'s existing
            // `Destroy | Disband` arm resolves it as a `Choose` (a bare
            // `Destroy` is illegal while the choice sits open) -- no
            // re-translation needed, no `claimed_destroy_lines` entry.
            // Otherwise the line is elsewhere (or a later one, since the
            // `weakest` selection runs before the event's other effects are
            // applied): drain it out of order from `lose_pop_destroys` --
            // the SAME per-player FIFO `ChoiceKind::LosePop` uses (both are
            // `"<Color> destroys <Card>"` lines, `prescan_lose_pop_
            // destroys`'s doc) -- validated against this choice's own
            // options, and recorded in `claimed_destroy_lines` exactly like
            // `LosePop` does so the main loop does not translate it a
            // second time. No journal-observed answer at all: guess the
            // first option (user-approved: guessing missing journal info is
            // fine as long as the math works out -- the destroy's cost
            // effects are fully determined by the engine).
            if matches!(self.state.pending.top(), Some(Pending::Choice(c)) if matches!(c.kind, ChoiceKind::DestroyOwn)) {
                let c = match self.state.pending.top().clone() {
                    Some(Pending::Choice(c)) => c,
                    _ => unreachable!("checked the top of the pending stack a line above"),
                };
                let matches_upcoming =
                    c.player == expected_actor && matches!(upcoming.0, ActionClass::Destroy | ActionClass::Disband);
                if !matches_upcoming {
                    // The choice's player is NOT `expected_actor` (or their
                    // upcoming line is unrelated): the answer is elsewhere in
                    // the journal, so it must be applied HERE, out of order,
                    // before `apply_one` is asked to translate a line that
                    // presupposes the choice is already closed.
                    let q = self.lose_pop_destroys.entry(c.player).or_default();
                    let n = loop {
                        let Some(&(line_idx, card)) = q.front() else {
                            break c
                                .options
                                .as_slice()
                                .iter()
                                .position(|o| matches!(o, ChoiceOption::Card(_)))
                                .ok_or_else(|| MismatchKind::StuckPending("DestroyOwn choice has no Card option to guess".into()))?;
                        };
                        if let Some(idx) = c
                            .options
                            .as_slice()
                            .iter()
                            .position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                        {
                            q.pop_front();
                            self.claimed_destroy_lines.insert(line_idx);
                            break idx;
                        }
                        // Belongs to this same player's own unrelated,
                        // separately-resolved voluntary destroy (see this
                        // block's own doc) -- not this choice's answer,
                        // skip past it and keep looking.
                        q.pop_front();
                    };
                    self.apply_move(Move::Choose { n: n as u8 });
                    // `interact::run_queue` only drains the queue while the
                    // pending stack is EMPTY -- a follow-on choice pushed by
                    // the `DestroyOwn` resolution (e.g. a second
                    // `destroyOwnBuilding`) would otherwise sit there, and
                    // the next `apply_move` would panic inside `apply::
                    // apply` (it is not an `Auction`). `run_queue` here is a
                    // no-op when nothing is open, so the cost is one branch.
                    crate::interact::run_queue(&mut self.state);
                    continue;
                }
                // `matches_upcoming` is true: the choice's player IS
                // `expected_actor` and the upcoming line IS a Destroy/Disband.
                // Return `Ok(())` immediately -- `apply_one`'s Destroy/Disband
                // arm resolves the choice by matching the upcoming line's
                // card against the options.
                return Ok(());
            }
            // A live `Pending::Colonize` has no real `Move` anywhere in the
            // journal vocabulary -- `docs/REPLAY.md`'s "gives up on"
            // section: this file always auto-drains the sacrifice
            // sequence, never grounds it to a real observed choice. Unlike
            // the `Pending::Choice` cases above there is therefore no
            // `matches_upcoming` escape hatch to check: `decider ==
            // expected_actor` does NOT mean "nothing left to resolve" the
            // way it does for a live political decision, it just means the
            // colonizer also happens to be up next for something else
            // entirely (their own Take/Build/... line, found on real games
            // `7523355`/`7523090`/`7523072` and 69 others in the corpus),
            // which cannot be legal while the colonize is still open. Drain
            // unconditionally, same as the pre-existing fallback below did
            // for the `decider != expected_actor` case -- this subsumes it.
            if matches!(self.state.pending.top(), Some(Pending::Colonize(_))) {
                self.drain_colonize()?;
                continue;
            }
            // A live `Pending::Auction` DOES have real `Move`s in the
            // journal (`Bid`/`BidPass`, `ActionClass::Bid`/`Pass`) -- but
            // only for the CURRENT decider's OWN upcoming response. When
            // `decider == expected_actor` and the upcoming line is one of
            // those, defer to it exactly like the `Pending::Choice` cases
            // above. Otherwise the auction still owes a decision from
            // `decider`, but the very next journal line is unrelated to it
            // entirely -- which only happens when that decision is FORCED
            // (their own `interact::max_force` ceiling no longer clears the
            // standing bid, so `BidPass` is their only legal move) and
            // BGO's UI auto-passes them with no click to log, the same
            // "forced, single legal option, no journal trace" shape as
            // `Pending::Defense`'s forced 0-defender `DefendDone` --
            // confirmed against real game `7523347` (a 4-way auction where
            // the second-to-last bidder's own concluding pass, having
            // already been outbid past their own ceiling, is never logged
            // at all). If more than `BidPass` is legally available here,
            // this is a genuine unexplained gap, not a forced pass -- this
            // binary must not silently pick a real decision for a human, so
            // it fails loudly instead of guessing.
            if matches!(self.state.pending.top(), Some(Pending::Auction(_))) {
                // Before ANY move is applied against this auction, whether
                // the journal's own bid/pass below or the forced auto-pass
                // -- either can be the one that settles it, and settling it
                // snapshots the winner's hand. See the method's own doc.
                self.ground_auction_winner_hand();
                let real_response = decider == expected_actor && matches!(upcoming.0, ActionClass::Bid | ActionClass::Pass);
                if !real_response {
                    let legal = legal::legal_moves(&self.state);
                    if legal.as_slice() == [Move::BidPass] {
                        self.apply_move(Move::BidPass);
                        continue;
                    }
                    return Err(MismatchKind::StuckPending(format!(
                        "auction decider {decider} owes a real bid/pass decision ({} legal moves) but \
                         the upcoming line ({:?}) is neither, and more than BidPass is legally available \
                         -- not a forced pass",
                        legal.as_slice().len(),
                        upcoming.0,
                    )));
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
                    self.apply_move(Move::Choose { n: n as u8 });
                }
                // Handled unconditionally above, before the `decider ==
                // expected_actor` check -- both `continue` or `return Err`
                // from there, so neither can still be on top by this point.
                Some(Pending::Auction(_)) => unreachable!("Pending::Auction is drained above this match"),
                Some(Pending::Colonize(_)) => unreachable!("Pending::Colonize is drained above this match"),
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
                        continue;
                    }
                    if decider != expected_actor && self.state.pending.is_empty() {
                        // The decider's political action (e.g. a hidden
                        // `PrepareEvent` for Development of Civil Life) may
                        // have just granted `expected_actor` a
                        // `one_time_discount`. The main loop's
                        // `civil_life_move` check ran BEFORE this political
                        // action was applied, so it missed the discount.
                        // Re-check here: if the upcoming line is explainable
                        // by the freshly-granted discount, apply it now via
                        // `apply_free_civil_move` and record the line so
                        // the main loop does not translate it a second time.
                        if let Some(mv) = civil_life_move(self, expected_actor, upcoming.0, upcoming.1) {
                            apply::apply_free_civil_move(&mut self.state, expected_actor, mv, 0);
                            self.claimed_civil_life_lines.insert(self.current_lineno);
                            return Ok(());
                        }
                        // An event's `destroy_own_building` with a SINGLE
                        // destroyable option is auto-resolved by
                        // `interact::push_choice` (its `auto` flag) during
                        // the political phase's `run_queue` -- the destroy
                        // is already applied, the choice never appears on
                        // the pending stack, and the journal's
                        // `"<Color> destroys <Card>"` line is a pure
                        // confirmation. Detect: the card is no longer in
                        // the player's buildings (or the line is a
                        // Disband of a unit that's already gone).
                        if matches!(upcoming.0, ActionClass::Destroy | ActionClass::Disband) {
                            let card = upcoming.1;
                            let p = &self.state.players[expected_actor as usize];
                            let already_gone = match card {
                                Some(cid) => !p.techs.has(cid),
                                None => false,
                            };
                            if already_gone {
                                self.claimed_civil_life_lines.insert(self.current_lineno);
                                return Ok(());
                            }
                        }
                    }
                    return Err(MismatchKind::StuckPending(format!(
                        "decider {decider} != expected actor {expected_actor}, phase {:?}, no pending",
                        self.state.phase
                    )));
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
        if crate::debugflags::replay_debug_all() {
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

        // FIX (found chasing 7522967's `SimulatorBug`/`PrepareEvent` ledger
        // bucket -- a `military_hand_deficit` under-repayment, the SAME
        // net-zero-push shape the comment below already fixed once, at a
        // DIFFERENT call site): snapshot every seat's `hand_military` length
        // HERE, before the grounding `push` two statements down, not after
        // it. The decider's own entry in `hand_military_len_before` (read
        // much further down, right before `self.apply_move(mv)`) is used to
        // measure how much the decider's hand GREW from this call so
        // `repay_military_hand_deficit` knows how much debt an `AllPlayers`/
        // `StrongestPlayer` immediate draw just repaid. Reading it AFTER the
        // grounding push below inflates the decider's own baseline by the
        // 1 phantom card that push adds (and `apply_move`'s own `Move::
        // PrepareEvent` removal cancels right back out) -- invisible whenever
        // nothing ELSE grows the decider's hand in the same `apply_move`
        // call, but for a revealed card whose own immediate effect ALSO
        // draws for the decider (Development of Politics, Politics of
        // Strength, ...), it undercounts that seat's real growth by exactly
        // one, so `repay_military_hand_deficit` credits one fewer unit than
        // the decider actually just drew -- deferring, not losing, the
        // correction, but leaving an extra card in the reconstructed hand
        // until whatever pays down the remainder. Snapshotting before ANY
        // mutation this function makes gives every seat, decider included,
        // its true pre-preparation length.
        let hand_military_len_before: [usize; MAX_PLAYERS] =
            std::array::from_fn(|i| self.state.players[i].hand_military.len());
        // The real player already held this card before preparing it: their
        // hand shrinks by exactly one (N -> N-1). Left alone, the `push`
        // immediately below followed by `apply`'s own removal of the same
        // identity once `Move::PrepareEvent` applies is a WASH (N -> N+1 ->
        // N), permanently overcounting this binary's own reconstructed hand
        // by one card per preparation -- see this file's "Discard-phase
        // hand-size oracle" section, `7522614`'s round-4 card-by-card trace.
        // Pop one card of UNKNOWN provenance first (never one
        // `DiscardSolver::needed_after` says this player is later observed
        // to play by name -- the same "never touch a card with known
        // identity" rule `ground_bid_ceiling`, just above, already applies
        // for the identical reason) so the whole sequence lands on N-1.
        // Leaves the old net-zero behaviour untouched -- not a regression,
        // an honest miss -- when no disposable filler exists.
        let needed_later = self.discard_solver.needed_after(decider, self.current_lineno);
        if let Some(&victim) = self.state.players[decider as usize]
            .hand_military
            .as_slice()
            .iter()
            .find(|id| !needed_later.contains(id.get().base_name))
        {
            self.state.players[decider as usize].hand_military.remove_first(victim);
        } else {
            // No disposable filler exists RIGHT NOW -- confirmed on the
            // task's own repro, game `7522634` round 3 decider 0: hand is
            // `["Legion"]`, but `needed_later` also contains `"Legion"` (the
            // discard solver has proven this exact identity is played by
            // name later), so it is not a genuinely empty hand, just a
            // fully-protected one -- the same dead end an actually-empty
            // hand reaches. Either way the push-then-apply-removal below
            // still washes to net zero unless something is done: the real
            // player's hand DID shrink by one here (a real card left their
            // hand to prepare this event), this binary's simulated one just
            // has no disposable stand-in to sacrifice for it yet. Record a
            // signed deficit instead of silently dropping the decrement --
            // repaid by [`Self::repay_military_hand_deficit`] the next time
            // this player's hand_military genuinely GROWS (a real draw),
            // which retroactively supplies the missing victim instead of
            // letting the drawn card inflate the hand on top of the debt.
            self.military_hand_deficit[decider as usize] += 1;
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
        // Only `Special::StrongestPlayers`/`WeakestPlayers` cards whose
        // `Gain`/`Lose` block carries a nonzero `food_and_or_resources` ever
        // reach `events::food_or_resources`'s deterministic split (Foray,
        // Raiders -- see `parse_produces_grant_line`'s doc comment for why
        // that split is provably wrong on real games). Gating the whole
        // correction below on the REVEALED CARD itself, not just an
        // after-the-fact delta match, is load-bearing: a delta-only gate
        // (an earlier version of this fix) matched on totals ALONE and
        // regressed the corpus (`IllegalMove: Pop` 184 -> 281) by
        // occasionally consuming an unrelated `ChoiceKind::GainBlock`
        // single-clause FIFO entry -- or even a stray same-total delta from
        // an entirely different simultaneous effect -- for a card that
        // never called `food_or_resources` at all.
        let triggers_food_or_resources = prep.revealed.get().special.iter().any(|sp| {
            matches!(sp, crate::cards::Special::StrongestPlayers(_) | crate::cards::Special::WeakestPlayers(_))
        }) && prep.revealed.get().special.iter().any(|sp| match sp {
            crate::cards::Special::Gain(b) | crate::cards::Special::Lose(b) => b.food_and_or_resources != 0,
            crate::cards::Special::A(_) | crate::cards::Special::AllPlayers(_) | crate::cards::Special::B(_) | crate::cards::Special::BestTheaterDoubleCulture | crate::cards::Special::BothPlayers(_) | crate::cards::Special::BuildDiscount(_) | crate::cards::Special::CancelledIfPartiesAttackEachOther | crate::cards::Special::CannotPlayAggressionOrWar | crate::cards::Special::CivilActionBackOnTechDevelop(_) | crate::cards::Special::CivilActionUpgradeUrbanBuildingToTheater | crate::cards::Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | crate::cards::Special::ColonyImmediateBonusApplies | crate::cards::Special::ColonyPermanentBonusTransfers | crate::cards::Special::ComboFoodDiscount(_) | crate::cards::Special::ComboResourceDiscount(_) | crate::cards::Special::Condition(_) | crate::cards::Special::CultureFirstColony(_) | crate::cards::Special::CultureIfTopTwoStrength(_) | crate::cards::Special::CultureOnLeaveEqualToLabResourceProduction | crate::cards::Special::CultureOnRevolution(_) | crate::cards::Special::CultureOnTechDevelop(_) | crate::cards::Special::CulturePerAdditionalColony(_) | crate::cards::Special::CulturePerCivilizationWithMoreCulture(_) | crate::cards::Special::CulturePerHappyFromTemplesTheatersWonders(_) | crate::cards::Special::CulturePerLabEqualToLevel | crate::cards::Special::CulturePerLibraryTheaterPair(_) | crate::cards::Special::CulturePerTheater(_) | crate::cards::Special::DecreasePopulation(_) | crate::cards::Special::DestroyUrbanBuildings(_) | crate::cards::Special::DoubleBestMine | crate::cards::Special::DoublesTacticBonusOfOneArmy | crate::cards::Special::ExtraHappyPerHappySource(_) | crate::cards::Special::FinalScoring(_) | crate::cards::Special::FreeCivilAction(_) | crate::cards::Special::FreePopIncreasePerTurn | crate::cards::Special::GainCulturePerLevelOfRemovedCard(_) | crate::cards::Special::GainFoodOrResources(_) | crate::cards::Special::GainResources(_) | crate::cards::Special::InfantryCountsAsCavalryForTactics | crate::cards::Special::LastRoundSubstitute(_) | crate::cards::Special::LeaderTakeCivilActionDiscount(_) | crate::cards::Special::LibraryDiscountsIfTheater | crate::cards::Special::MilitaryActionAsCivilPerTurn(_) | crate::cards::Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | crate::cards::Special::NoAttacksBetweenParties | crate::cards::Special::OnAttackBetweenParties(_) | crate::cards::Special::OnBuildCulture(_) | crate::cards::Special::OnBuildCulturePerTechLevelSum | crate::cards::Special::OnReplacePutUnderCompletedWonderHappy(_) | crate::cards::Special::OncePerGameTwoPoliticalActions | crate::cards::Special::OpponentDecreasesPopulation(_) | crate::cards::Special::OpponentsPayDoubleMilitaryActionsToAttackYou | crate::cards::Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | crate::cards::Special::PeekTopEventCardInPolitics | crate::cards::Special::PerTurnChoice | crate::cards::Special::PlayerWithLeastCulture(_) | crate::cards::Special::PlayerWithMostCulture(_) | crate::cards::Special::PlayersWithMostDiscontentWorkers(_) | crate::cards::Special::PlayersWithMostHappyFaces(_) | crate::cards::Special::PopIncreaseFoodDiscount(_) | crate::cards::Special::RemoveAsPoliticalActionForYellowToken(_) | crate::cards::Special::RemoveAsPoliticalActionFreeColonize | crate::cards::Special::RemoveFromGame | crate::cards::Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | crate::cards::Special::ResourceOnTechDevelop(_) | crate::cards::Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | crate::cards::Special::ResourcesPerLabEqualToLevel | crate::cards::Special::RevolutionUsesMilitaryActionsInstead | crate::cards::Special::ScienceOnTechCardTake(_) | crate::cards::Special::SciencePerBestLabOrLibraryLevel | crate::cards::Special::SciencePerLab(_) | crate::cards::Special::StealColony(_) | crate::cards::Special::StrengthPerArtillery(_) | crate::cards::Special::StrengthPerInfantry(_) | crate::cards::Special::StrengthPerMilitaryUnit(_) | crate::cards::Special::StrengthPerTempleOrGovernmentHappy(_) | crate::cards::Special::StrengthPerUnitType(_) | crate::cards::Special::StrongestPlayer(_) | crate::cards::Special::StrongestPlayers(_) | crate::cards::Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | crate::cards::Special::TakeFromOpponent(_) | crate::cards::Special::TheaterResourceDiscountIfLibrary(_) | crate::cards::Special::TheaterScienceDiscountIfLibrary(_) | crate::cards::Special::TheaterTechScienceDiscount(_) | crate::cards::Special::VictorTakesCulture | crate::cards::Special::VictorTakesScienceUpTo(_) | crate::cards::Special::VictorTakesYellowTokens | crate::cards::Special::WeakestPlayer(_) | crate::cards::Special::WeakestPlayers(_) | crate::cards::Special::WonderTakeNoExtraCivilActions => false,
        });
        let n = self.state.num_players;
        let pre_food_resources: Vec<(u16, u16)> = if triggers_food_or_resources {
            (0..n).map(|i| (self.state.players[i as usize].food, self.state.players[i as usize].resources)).collect()
        } else {
            Vec::new()
        };
        // TOKENCOUNT fix (2026-08-14): the correction below used to overwrite
        // `p.food`/`p.resources` directly, leaving `p.food_tokens`/
        // `p.resource_tokens` (the per-denomination placement history --
        // `crate::state::TokenBank`'s own doc) holding whatever
        // `events::food_or_resources`'s own default guessed split had
        // already credited/debited through the real `gain_food`/
        // `gain_resources`/`pay_food`/`pay_resources` chokepoints. Once the
        // scalar is corrected but the bank is not, `economy::occupied_tokens`
        // either silently drops the excess (corrected total BELOW the
        // guessed-and-tracked value: it only tops up on an INCREASE, see its
        // own doc) or re-packs a phantom top-up on top of an already-wrong
        // bank (corrected total ABOVE it) -- either way the tracked token
        // COUNT permanently diverges from a real single consistent
        // placement history, even though the corrected VALUE is right. Snapshot
        // the bank here too, so the correction below can roll the GUESSED
        // gain/loss back out through the same blessed chokepoints before
        // applying the JOURNAL's own true split -- keeping bank and scalar
        // in lockstep the same way every other gain/loss site already must.
        let pre_food_tokens: Vec<crate::state::TokenBank> = if triggers_food_or_resources {
            (0..n).map(|i| self.state.players[i as usize].food_tokens).collect()
        } else {
            Vec::new()
        };
        let pre_resource_tokens: Vec<crate::state::TokenBank> = if triggers_food_or_resources {
            (0..n).map(|i| self.state.players[i as usize].resource_tokens).collect()
        } else {
            Vec::new()
        };
        // BUG (found chasing the 103-game `SimulatorBug`/`PrepareEvent`
        // ledger bucket, simulator excess = journal excess + 1 in 81 of
        // those 103 -- the classic unpaid-deficit shape): every OTHER
        // caller that can grow a seat's `hand_military` for real routes
        // through `Self::try_apply`, whose own post-`apply::apply` loop
        // calls `Self::repay_military_hand_deficit` for every seat that
        // just grew (see that function's own doc). This function calls
        // `apply::apply` DIRECTLY -- the one remaining call site that
        // does not -- because `events::reveal_current_event`, invoked
        // synchronously inside THIS SAME `apply::apply` call via
        // `apply::h_prepare_event`, can immediately resolve the newly
        // revealed card's own effect, which for an `AllPlayers`/
        // `StrongestPlayer`/etc. `drawMilitaryCards` block (Development of
        // Politics, Politics of Strength, ...) pushes real cards straight
        // into one or more players' `hand_military` -- INCLUDING the
        // decider's own, e.g. right after THIS SAME preparation's "no
        // disposable filler" branch just above recorded a fresh debt for
        // them. Skipping the growth-check here means that draw can never
        // repay any deficit -- not the decider's fresh one, and not any
        // other seat's pre-existing one either, since an `AllPlayers`
        // block draws for every seat in one call -- so the debt stays on
        // the books forever while the drawn card(s) sit uncompensated in
        // hand: a permanent phantom card, not the temporary window the
        // deficit mechanism is supposed to guarantee. `hand_military_len_
        // before` is snapshotted at the TOP of this function now (see the
        // comment there for why it must run before the grounding push
        // above, not here), and repaid against growth after `apply_move`
        // below, mirroring `try_apply`'s own loop exactly.
        // TIE_CENSUS labelling only: `current_lineno` tracks the outer
        // per-journal-line loop, but THIS call (a political decision) is not
        // driven from that loop at all -- it can run before that line's own
        // journal row is reached. `prep.lineno` is the journal row this
        // exact preparation/reveal was solved from (`event_plan::solve`),
        // the one whose own "Current event:" clause names the real outcome
        // if `events::reveal_current_event` resolves synchronously inside
        // this same `apply::apply` call (see the big comment above).
        crate::tie_context::set_lineno(prep.lineno);
        self.apply_move(mv);
        for seat in 0..self.state.num_players {
            let before = hand_military_len_before[seat as usize];
            let after = self.state.players[seat as usize].hand_military.len();
            if after > before {
                self.repay_military_hand_deficit(seat, (after - before) as u32);
            }
        }

        // The engine just resolved the split ITS OWN way -- these days a
        // real per-player `ChoiceKind::FoodOrRes` decision rather than the
        // old fixed "resources first" formula, but either way it is the
        // BOT's choice and not the human's. Correct every player whose
        // food/resources changed against the journal's OWN resolution line
        // for that player, popped from `produces_grants` -- gated ABOVE on
        // the revealed card actually being this shape, and HERE on the
        // popped entry's own total matching the delta this apply just
        // produced, so a same-shaped-but-unrelated FIFO entry (a genuine
        // `ChoiceKind::GainBlock` single-clause pick sitting in the same
        // per-seat queue, see that parser's own doc) can only be mistaken
        // for this correction on a coincidental total match while ALSO
        // landing on a turn where a real Foray/Raiders fired -- and even
        // then, the result is still a real, journal-observed split for that
        // player, just attributed to the wrong event.
        //
        // REPLAYER BUG (found chasing `IllegalMove: Pop`'s "food short by
        // 1/2/3, cost tier right" signature, `docs/REPLAY.md`'s handoff --
        // game `7523052` round 9): `prescan_produces_grants` fills this same
        // per-seat queue from EVERY standalone `"<Color> produces N food[;
        // ...]"` line in the whole journal, not just the ones this gated
        // correction ever consumes -- an `AllPlayers`-shaped grant like
        // Development of Markets ("gains 2 resources or 2 food, player's
        // choice") resolves through a real `Pending::Choice`
        // (`ChoiceKind::FoodOrRes`) that never touches `produces_grants` at
        // all, so its own `"Purple produces 2 food"` line sits at the FRONT
        // of Purple's queue forever once queued. Only ever PEEKING the
        // front entry (the previous version of this loop) meant that one
        // foreign entry permanently blocked every REAL Foray/Raiders
        // correction for that player for the rest of the game, silently
        // falling back to the (frequently wrong) default split every time --
        // confirmed on `7523052`: Foray's real `"Purple produces 1 food;
        // Purple produces 2 resources"` (round 9) never applied because an
        // unrelated `(2, 0)` entry from round 5's Development of Markets sat
        // in front of it with a non-matching total (2 vs this grant's 3).
        // Scanning forward for the first entry whose OWN total matches
        // (removing it from wherever it sits, not just the front) instead
        // skips past a foreign entry exactly the way the sibling
        // `PlunderSplit` consumer already does for the same reason (see
        // `prescan_plunder_splits`'s own doc) -- leaving it un-popped and
        // harmless (nothing else ever reads this queue) rather than letting
        // it block every real entry queued behind it.
        if triggers_food_or_resources {
            for i in 0..n {
                let (pre_food, pre_res) = pre_food_resources[i as usize];
                let post = &self.state.players[i as usize];
                let delta_food = post.food as i32 - pre_food as i32;
                let delta_res = post.resources as i32 - pre_res as i32;
                // `events::food_or_resources` never mixes signs within one
                // call (its gain arm only ever adds, its loss arm only ever
                // subtracts -- see that function's own body), so a real
                // correction candidate is either both deltas >= 0 (a
                // `StrongestPlayers` gain, corrected against
                // `produces_grants`) or both <= 0 (a `WeakestPlayers` loss,
                // corrected against `spends_grants`, added here -- see that
                // FIFO's own doc for why the ORIGINAL version of this loop,
                // which unconditionally skipped every negative delta, only
                // ever corrected the gain half).
                if delta_food > 0 || delta_res > 0 {
                    let total = delta_food + delta_res;
                    if let Some(q) = self.produces_grants.get_mut(&i) {
                        if let Some(pos) = q.iter().position(|&(jf, jr)| jf as i32 + jr as i32 == total) {
                            let (jf, jr) = q.remove(pos).expect("position just found by iter()");
                            let p = &mut self.state.players[i as usize];
                            apply_journal_food_or_res_correction(
                                p,
                                pre_food,
                                pre_res,
                                pre_food_tokens[i as usize],
                                pre_resource_tokens[i as usize],
                                jf,
                                jr,
                                false,
                            );
                        }
                    }
                } else if delta_food < 0 || delta_res < 0 {
                    let total = -(delta_food + delta_res);
                    if let Some(q) = self.spends_grants.get_mut(&i) {
                        if let Some(pos) = q.iter().position(|&(jf, jr)| jf as i32 + jr as i32 == total) {
                            let (jf, jr) = q.remove(pos).expect("position just found by iter()");
                            let p = &mut self.state.players[i as usize];
                            apply_journal_food_or_res_correction(
                                p,
                                pre_food,
                                pre_res,
                                pre_food_tokens[i as usize],
                                pre_resource_tokens[i as usize],
                                jf,
                                jr,
                                true,
                            );
                        }
                    }
                }
            }
        }

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

        if crate::debugflags::replay_debug_all() {
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

    /// Raise `actor`'s colonization ceiling to at least `n` by grounding
    /// military bonus cards into the SIMULATED filler in their hand, so the
    /// bid the journal records as legal actually is. Returns whether it
    /// could -- `false` leaves the caller's honest mismatch report intact.
    ///
    /// §11.2 caps a bid at the bidder's own maximum colonization force, and
    /// BGO enforces that cap in its own client: a human cannot click a bid
    /// they could not pay. A logged `"<Color> bids N"` is therefore a
    /// JOURNAL FACT about a hand this binary cannot see -- their max force
    /// was at least `N` -- in exactly the sense a `"Defense card +6 played"`
    /// clause is a journal fact about their hand. Nothing about it is a
    /// guess, and nothing here uses private information: the bid is public,
    /// shouted at the table.
    ///
    /// What IS a choice is which filler cards to overwrite and with what,
    /// and this keeps that claim as small as the fact allows:
    ///
    /// - Only cards already in hand are converted, never added. The hand's
    ///   SIZE is modelled exactly (every draw and discard is logged), so
    ///   growing it to explain a bid would trade a known fact for a guess.
    ///   Running out of filler is the honest `false`.
    /// - Only cards `DiscardSolver::needed_after` does not rule out -- an
    ///   identity the journal later shows this player PLAYING is one of the
    ///   few hand slots that is not filler at all.
    /// - Fewest cards, then smallest printed value: the smallest bonus that
    ///   closes the gap outright, else the largest available (which is what
    ///   makes progress). Every extra or larger card claimed would be a
    ///   detail about a hidden hand that the bid does not actually pin down.
    /// - Never a card newer than the military deck's own current age. A
    ///   player can hold an OLD bonus card, never a future one.
    fn ground_bid_ceiling(&mut self, actor: u8, n: u8) -> bool {
        let mut values: Vec<(i16, CardId)> = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .filter(|id| id.kind() == CardType::Bonus && id.get().age <= self.state.age_military)
            .map(|id| (id.get().effects.colonization_bonus, id))
            .collect();
        values.sort_unstable_by_key(|&(value, _)| value);
        if values.is_empty() {
            return false;
        }
        let needed_later = self.discard_solver.needed_after(actor, self.current_lineno);
        for _ in 0..MAX_HAND {
            let ceiling = crate::interact::max_force(&self.state, &self.state.players[actor as usize]);
            let short = n as i32 - ceiling;
            if short <= 0 {
                return true;
            }
            // Worst defender first, the same "burn the least valuable card"
            // convention `interact::discard_options` and `cook_pool` sort by
            // -- a filler slot has no identity of its own, so the only
            // meaningful ordering is which real card it would hurt least to
            // turn out not to have been.
            let mut filler: Vec<CardId> = self.state.players[actor as usize]
                .hand_military
                .as_slice()
                .iter()
                .copied()
                .filter(|id| id.kind() != CardType::Bonus && !needed_later.contains(id.get().base_name))
                .collect();
            filler.sort_by_key(|id| (crate::interact::defense_points(*id), id.name()));
            let Some(&victim) = filler.first() else { return false };
            let (_, replacement) = values
                .iter()
                .find(|&&(value, _)| value as i32 >= short)
                .copied()
                .unwrap_or_else(|| *values.last().expect("checked non-empty above"));
            let hand = &mut self.state.players[actor as usize].hand_military;
            hand.remove_first(victim);
            hand.push(replacement);
            self.bid_ceilings_grounded += 1;
        }
        false
    }

    /// Ground the bonus cards the journal says the auction currently on top
    /// is about to be won with, BEFORE the winning bid/pass is applied.
    ///
    /// The timing is forced by the engine, not chosen here: `interact::
    /// auction_move` runs the whole colonization synchronously the moment
    /// the auction settles, and `interact::colonize` SNAPSHOTS the winner's
    /// military hand into `Pending::Colonize::bpool` at that instant. A
    /// bonus card grounded any later than this could never be sent, so the
    /// engine would be forced to make up the difference out of army units
    /// the human never spent.
    ///
    /// Only fires when the queue front really belongs to THIS auction --
    /// its actor is still a live bidder and its territory family matches the
    /// card being auctioned. An auction every player passes produces no
    /// `"colonizes"` line at all, and without both checks that missing entry
    /// would silently hand a later auction's winning cards to somebody now.
    fn ground_auction_winner_hand(&mut self) {
        let Some(Pending::Auction(a)) = self.state.pending.top() else { return };
        let Some(sac) = self.colonize_sacrifices.front() else { return };
        // `base_name`, not `name`: the card table's `name` carries the age
        // suffix (`"Vast Territory (I)"`) that BGO's journal never prints.
        if !a.active().contains(&sac.actor) || a.card.get().base_name != sac.territory {
            return;
        }
        let actor = sac.actor;
        let mut needed: HashMap<CardId, usize> = HashMap::new();
        for clause in &sac.clauses {
            if let SacrificeClause::Bonus(id) = clause {
                *needed.entry(*id).or_default() += 1;
            }
        }
        // Same net-zero shape as `ground_for_consumption` (see its own doc,
        // just above): each phantom `push` below is consumed for real once
        // `drain_colonize`'s own `Move::SendBonus` removes this identical
        // identity from `hand_military` (`interact::auction_move`'s
        // `Move::SendBonus` arm, `hand_military.remove_first(card)`). A real
        // auction winner who sacrifices a bonus card they never revealed
        // beforehand already HELD it -- their hand shrinks by one per card
        // sacrificed, not zero. Pop one filler of unknown provenance per
        // phantom push (never a card this same sacrifice still needs --
        // cannibalizing one `needed` entry to manufacture another would just
        // relocate the phantom, not remove it -- and never one
        // `DiscardSolver::needed_after` says this player is later observed
        // playing by name, the same rule `ground_bid_ceiling`/
        // `ground_for_consumption` already use) so each phantom lands on
        // N -> N-1, not N -> N.
        //
        // FIX: when no disposable filler exists -- every card in hand is
        // either something this same sacrifice still needs or something
        // `needed_later` has already proven is played by name -- there is
        // nothing to evict, and this used to just `push` anyway with no
        // compensating decrement recorded anywhere. `Move::SendBonus` still
        // removes the pushed identity for real right after, so the pair
        // nets to zero instead of the -1 an auction winner who genuinely
        // held and sacrificed the card would leave behind: a PERMANENT
        // phantom card, not the transient wash the comment above describes.
        // Record the same `military_hand_deficit` fallback
        // `ground_for_consumption` records for its own no-filler case, so
        // `repay_military_hand_deficit` settles this debt too on the actor's
        // next genuine draw instead of it staying owed forever.
        let needed_later = self.discard_solver.needed_after(actor, self.current_lineno);
        for (&card, &count) in &needed {
            let have = self.state.players[actor as usize]
                .hand_military
                .as_slice()
                .iter()
                .filter(|&&id| id == card)
                .count();
            for _ in have..count {
                let victim = self.state.players[actor as usize]
                    .hand_military
                    .as_slice()
                    .iter()
                    .copied()
                    .find(|id| !needed.contains_key(id) && !needed_later.contains(id.get().base_name));
                if let Some(victim) = victim {
                    self.state.players[actor as usize].hand_military.remove_first(victim);
                } else {
                    self.military_hand_deficit[actor as usize] += 1;
                }
                self.state.players[actor as usize].hand_military.push(card);
            }
        }
    }

    /// Resolve the open `Pending::Colonize` against the journal's own
    /// `"Sacrificed Units:; ..."` list -- one `Move::SendUnit` /
    /// `Move::SendBonus` / `Move::SendDiscard` per clause, then
    /// `Move::SendDone`.
    ///
    /// Falls back to [`Replayer::approximate_colonize`] (and records the
    /// game as approximated) whenever the journal's next piece is not a
    /// legal continuation here -- which means this binary's reconstruction
    /// of the colonizer's army or hand has already diverged, so forcing the
    /// move would be faking state rather than replaying it.
    fn drain_colonize(&mut self) -> Result<(), MismatchKind> {
        let Some(Pending::Colonize(c)) = self.state.pending.top() else { return Ok(()) };
        let player = c.player;
        let Some(sac) = self.colonize_sacrifices.front() else {
            if crate::debugflags::colonize_fallback_dump() {
                eprintln!("COLONIZE_FALLBACK reason=queue-empty actor={player} line={}", self.current_lineno);
            }
            return self.approximate_colonize();
        };
        if sac.actor != player {
            if crate::debugflags::colonize_fallback_dump() {
                eprintln!(
                    "COLONIZE_FALLBACK reason=wrong-actor sac_actor={} player={player} line={}",
                    sac.actor, sac.lineno
                );
            }
            return self.approximate_colonize();
        }
        let sac = self.colonize_sacrifices.pop_front().expect("just peeked");
        for _ in 0..64 {
            let Some(Pending::Colonize(c)) = self.state.pending.top() else { return Ok(()) };
            // What the journal still owes, as a multiset difference against
            // what `colonize_auto` has already forced in on its own.
            let mut owed: Vec<SacrificeClause> = sac.clauses.clone();
            for &id in c.units.as_slice() {
                remove_first_clause(&mut owed, SacrificeClause::Unit(id));
            }
            for &id in c.bonuses.as_slice() {
                remove_first_clause(&mut owed, SacrificeClause::Bonus(id));
            }
            for _ in 0..c.discards.len() {
                remove_first_clause(&mut owed, SacrificeClause::CookDiscard);
            }
            // §11.3's "at least one unit" floor is a floor on the SACRIFICE,
            // and `interact::colonize_moves` offers nothing but units until
            // it is met -- so a unit always has to go first when none has
            // been committed yet.
            let next = if c.units.is_empty() {
                owed.iter().find(|cl| matches!(cl, SacrificeClause::Unit(_))).copied()
            } else {
                owed.first().copied()
            };
            // The journal NAMES the sacrificed unit. When it is not yet a
            // legal `Move::SendUnit` continuation, that is our own ARMY
            // reconstruction missing it or having it parked on the wrong
            // tech card -- not the human making an illegal move. Ground it
            // the same way `ground_auction_winner_hand` already grounds a
            // colonization's BONUS cards ahead of time; see
            // `ground_army_unit_for_consumption`'s own doc for why this is a
            // sound reconstruction repair, not a fabricated move.
            if let Some(SacrificeClause::Unit(card)) = next {
                self.ground_army_unit_for_consumption(player, card, &owed);
            }
            let legal = legal::legal_moves(&self.state);
            let mv = match next {
                Some(SacrificeClause::Unit(card)) => Move::SendUnit { card },
                Some(SacrificeClause::Bonus(card)) => Move::SendBonus { card },
                // Cook's discard names no card; any candidate the engine
                // still offers is equally consistent with the journal, which
                // recorded only that a discard happened.
                Some(SacrificeClause::CookDiscard) => {
                    match legal.as_slice().iter().find(|m| matches!(m, Move::SendDiscard { .. })) {
                        Some(&m) => m,
                        None => {
                            if crate::debugflags::colonize_fallback_dump() {
                                eprintln!(
                                    "COLONIZE_FALLBACK reason=cook-discard-unavailable actor={player} line={}",
                                    sac.lineno
                                );
                            }
                            return self.approximate_colonize();
                        }
                    }
                }
                None => Move::SendDone,
            };
            if !legal.as_slice().contains(&mv) {
                if crate::debugflags::colonize_fallback_dump() {
                    eprintln!(
                        "COLONIZE_FALLBACK reason=illegal-move actor={player} line={} clause={next:?} mv={mv:?}",
                        sac.lineno
                    );
                }
                return self.approximate_colonize();
            }
            self.apply_move(mv);
        }
        Err(MismatchKind::StuckPending(format!(
            "colonize sacrifice from line {} did not resolve in 64 steps",
            sac.lineno
        )))
    }

    /// Ground the journal's own NAMED colonize-sacrifice unit into this
    /// binary's reconstructed army when [`Replayer::drain_colonize`] cannot
    /// yet offer it as a legal [`Move::SendUnit`] -- the ARMY-side
    /// counterpart of [`Replayer::ground_for_consumption`]: swap one worker
    /// off whichever OTHER unit this binary's own reconstruction currently
    /// has available onto `card` instead. Total army size (workers on unit
    /// tech cards) is UNCHANGED -- this only ever corrects WHICH unit types
    /// this binary believes make up the army, never how many units exist.
    /// The journal naming a unit as sacrificed is itself proof the human
    /// held it; injecting it here is exactly the "the journal is a stream of
    /// facts about the hidden state, use it" repair this file's other
    /// grounding helpers already make for `hand_military`, applied to the
    /// army instead.
    ///
    /// Never picks a victim `owed` still needs for a LATER clause of this
    /// same sacrifice (mirrors `ground_auction_winner_hand`'s identical
    /// caution for bonus cards) -- stealing a unit this same colonization is
    /// about to need again would just relocate the gap, not close it.
    ///
    /// DELIBERATELY REFUSES to ground when no such victim exists, rather
    /// than growing the army by an ungrounded worker: a grant-without-a-
    /// victim variant was tried and measured (`colofix__COLOFIX.txt`) --
    /// it fixed one more fallback fire but net REGRESSED the corpus by
    /// inflating a player's worker count past what an unrelated, otherwise
    /// correctly-reconstructed later `ChoiceKind::LosePop` destroy-choice
    /// (game 7522866) expected, permanently stalling that replay. A no-op
    /// here just falls through to the pre-existing
    /// [`Replayer::approximate_colonize`] fallback, unchanged from before
    /// this repair existed -- strictly no worse than the baseline.
    fn ground_army_unit_for_consumption(&mut self, actor: u8, card: CardId, owed: &[SacrificeClause]) {
        let Some(Pending::Colonize(c)) = self.state.pending.top_mut() else { return };
        if c.pool.contains(card) {
            return;
        }
        let Some(victim) = c
            .pool
            .as_slice()
            .iter()
            .copied()
            .find(|&id| id != card && !owed.contains(&SacrificeClause::Unit(id)))
        else {
            return;
        };
        if crate::debugflags::colonize_fallback_dump() {
            eprintln!(
                "GROUND_ARMY_SWAP actor={actor} card={card:?} victim={victim:?} pool_before={:?}",
                c.pool.as_slice()
            );
        }
        c.pool.remove_first(victim);
        c.pool.push(card);
        let p = &mut self.state.players[actor as usize];
        if let Some(slot) = p.techs.get_mut(victim) {
            slot.workers = slot.workers.saturating_sub(1);
        }
        match p.techs.get_mut(card) {
            Some(slot) => slot.workers += 1,
            None => p.techs.insert(card, TechSlot { workers: 1, stored: 0 }),
        }
    }

    /// Repeatedly pick the engine's own first-offered continuation of an
    /// open `Pending::Colonize` until it clears -- the fallback for a
    /// colonization whose journal record [`Replayer::drain_colonize`] could
    /// not follow. Records that this game's colonize sacrifice is
    /// approximate.
    fn approximate_colonize(&mut self) -> Result<(), MismatchKind> {
        self.colonize_approximated = true;
        for _ in 0..64 {
            if !matches!(self.state.pending.top(), Some(Pending::Colonize(_))) {
                return Ok(());
            }
            let legal = legal::legal_moves(&self.state);
            let Some(&mv) = legal.as_slice().first() else {
                return Err(MismatchKind::StuckPending("Pending::Colonize offered zero moves".into()));
            };
            self.apply_move(mv);
        }
        Err(MismatchKind::StuckPending("colonize force did not resolve in 64 steps".into()))
    }

    /// Ground `card` into `actor`'s military hand -- push it if the journal
    /// is the first place this binary has ever seen it, popping one card of
    /// UNKNOWN provenance first (never one `DiscardSolver::needed_after`
    /// says this player is later observed playing by name -- the same rule
    /// `ground_bid_ceiling`, above, already uses) so the push nets N -> N-1,
    /// not N -> N+1. A no-op if `card` is already present (e.g. the
    /// fictional per-round deal happened to match).
    ///
    /// DELIBERATELY LOW-LEVEL AND PRIVATE. This function grounds a card but
    /// does NOT consume it -- on its own it is exactly the "half of the
    /// pair" shape that caused the bug this file is named for (`docs/
    /// REPLAY.md`'s "Discard-phase hand-size oracle" section): call this
    /// alone, without the very next state-mutating step being the `Move`
    /// that removes this identical identity from `hand_military`, and the
    /// grounding either overcounts (if nothing ever consumes it) or -- if
    /// something eventually does, but not the very next thing -- reproduces
    /// the same net-zero wash `PrepareEvent`'s push-then-apply-removal had,
    /// just with more code in between to hide it.
    ///
    /// [`Replayer::consume_named_military_card`] is the sanctioned way to
    /// ground-and-play a card in one atomic step; USE THAT, not this,
    /// unless you are one of the two callers that cannot: `ColumbusColonize`
    /// (an unavoidable `resolve_intervening` step sits between the ground
    /// and the consuming `Move::ColumbusColonize`) and `ground_auction_
    /// winner_hand` (grounds a whole BATCH of cards ahead of a `Move::
    /// SendBonus` that may not fire until several journal lines later, once
    /// `drain_colonize` gets to it) -- both are doc'd at their own call
    /// sites as the reason they cannot use the wrapper.
    ///
    /// FIX (found chasing the corpus's dominant `hand_military_excess`
    /// drift, 481 games at delta +1 -- `docs/REPLAY.md`'s "Discard-phase
    /// hand-size oracle" section): when NO disposable filler exists (every
    /// card currently in hand is one `DiscardSolver::needed_after` has
    /// already proven this player plays by name later -- the exact
    /// "fully-protected, not merely empty" hand `resolve_political_
    /// decision`'s own `PrepareEvent` handling names and fixes for itself),
    /// this used to just `push` anyway with no compensating eviction: the
    /// consuming `Move` right after removes the same identity again, so the
    /// whole call nets to zero instead of the real -1 a played card leaves
    /// behind, with NO debt recorded anywhere -- a PERMANENT phantom card,
    /// unlike `PrepareEvent`'s own wash which at least gets a
    /// `military_hand_deficit` entry a later draw can repay. Record the
    /// same signed deficit here so every no-filler wash in the file shares
    /// one repayment path instead of the two most-travelled call sites
    /// (`PlayTactic`/`DeclareWar`/`PlayAggression`/`ProposePact`/defense,
    /// all routed through [`Replayer::consume_named_military_card`], plus
    /// `ColumbusColonize`'s own direct call) silently defeating it.
    fn ground_for_consumption(&mut self, actor: u8, card: CardId) {
        let hand = &self.state.players[actor as usize].hand_military;
        if hand.contains(card) {
            return;
        }
        let needed_later = self.discard_solver.needed_after(actor, self.current_lineno);
        let victim = self.state.players[actor as usize]
            .hand_military
            .as_slice()
            .iter()
            .find(|id| !needed_later.contains(id.get().base_name))
            .copied();
        if let Some(victim) = victim {
            self.state.players[actor as usize].hand_military.remove_first(victim);
        } else {
            self.military_hand_deficit[actor as usize] += 1;
        }
        self.state.players[actor as usize].hand_military.push(card);
    }

    /// Ground `card` into `actor`'s military hand ([`Replayer::
    /// ground_for_consumption`]) and immediately apply the `Move` that
    /// consumes that identical identity -- the ONE call every `ActionClass`
    /// arm that reveals-and-plays a named military card (`PlayTactic`/
    /// `DeclareWar`/`PlayAggression`/`ProposePact`) and `resolve_aggression_
    /// defense`'s own committed-card clauses should make. Bundling the
    /// grounding with the consuming `Move` into a single non-decomposable
    /// call is what makes the net-zero wash this file used to have
    /// structurally impossible to reintroduce at these call sites: there is
    /// no longer a bare grounding step for a future edit to leave stranded
    /// without its consuming `Move` attached (which is exactly how the
    /// original bug read at each of the four `ActionClass` arms -- `docs/
    /// REPLAY.md`'s "Discard-phase hand-size oracle" section, and see
    /// `resolve_political_decision`'s own `PrepareEvent` fix for the first
    /// occurrence of this exact shape).
    fn consume_named_military_card(&mut self, actor: u8, card: CardId, mv: Move, record: bool) -> Result<(), MismatchKind> {
        self.ground_for_consumption(actor, card);
        self.try_apply(mv, record)
    }

    /// Drains up to `growth` units of `actor`'s [`Self::military_hand_
    /// deficit`] by popping one already-simulated filler card per unit
    /// repaid -- called from [`Self::try_apply`] right after `apply::apply`
    /// for every seat whose `hand_military` just grew for real (a genuine
    /// draw, e.g. `Move::EndTurn`'s own end-of-turn draw), so that growth
    /// pays back an earlier `resolve_political_decision` "no disposable
    /// filler exists" wash instead of stacking a phantom extra card on top
    /// of it. Never pops a card `DiscardSolver::needed_after` says this
    /// seat is later observed playing by name, the same rule every other
    /// filler pop in this file already follows. A no-op when there is no
    /// deficit, the hand did not grow, or every remaining card is
    /// protected (the deficit then stays owed, exactly like the original
    /// wash -- this narrows the window the phantom card can exist in, it
    /// does not claim to close it in every case).
    fn repay_military_hand_deficit(&mut self, actor: u8, growth: u32) {
        let owed = self.military_hand_deficit[actor as usize].min(growth);
        if owed == 0 {
            return;
        }
        let needed_later = self.discard_solver.needed_after(actor, self.current_lineno);
        for _ in 0..owed {
            let hand = &mut self.state.players[actor as usize].hand_military;
            // Prefer a victim that is NOT `CardType::Event`/`Territory`
            // (the "harp symbol" cards RULES_SPEC 5.2 reserves specifically
            // for paying a future `Move::PrepareEvent`'s own cost) over one
            // that is, falling back to an Event/Territory card only when it
            // is the sole unprotected option left. This debt has no rule
            // tying it to any particular card type -- it exists purely to
            // correct an earlier accounting shortfall (`ground_for_
            // consumption`/`ground_auction_winner_hand`'s own no-filler
            // case) -- so spending it on a harp-symbol card is never
            // required, and doing so anyway can strand a LATER, real
            // preparation with nothing rule-legal left to pay with even
            // though this reconstruction's hand still (mis)counts enough
            // cards overall. Confirmed on game `7523376`: the round-10
            // repay firing before round-11's own "International Agreement"
            // preparation grounding picks the SAME `CardId::Event` card
            // (`"Ravages of Time"`) the preparation would have needed,
            // forcing the preparation to fall back to an ineligible
            // Aggression card instead and drift the hand permanently short.
            let victim = hand
                .as_slice()
                .iter()
                .find(|id| !needed_later.contains(id.get().base_name) && !matches!(id.get().kind, CardType::Event | CardType::Territory))
                .or_else(|| hand.as_slice().iter().find(|id| !needed_later.contains(id.get().base_name)));
            let Some(&victim) = victim else {
                break;
            };
            hand.remove_first(victim);
            self.military_hand_deficit[actor as usize] -= 1;
        }
    }

    /// Cross-checks this binary's own reconstructed military-hand excess for
    /// `actor` against [`discard_phase_oracle`](Self::discard_phase_oracle)'s
    /// cross-validated journal truth for the exact `(actor, line.round)`
    /// checkpoint -- see this file's "Discard-phase hand-size oracle" module
    /// doc. Called from the `EndTurn` dispatch arm right AFTER
    /// `resolve_intervening` and right BEFORE `try_apply(Move::EndTurn, ..)`
    /// -- the one point in the whole replay where `self.state.players[actor]
    /// .hand_military` is guaranteed to be exactly what `interact::
    /// discard_excess_military` (step 1 of `economy::end_of_turn`, the very
    /// next thing that runs) is about to read, mirroring that function's own
    /// `limit` formula exactly (`military_actions + military_hand_limit`).
    ///
    /// A silent no-op when the oracle has no trusted entry for this
    /// checkpoint -- most commonly the `game_over`-guarded duplicate
    /// trailing "End turn" line for the true final turn (this function is
    /// not even called on that path, see the call site), or a round whose
    /// two journal renderings disagreed with EACH OTHER and so were dropped
    /// by `prescan_discard_phase_oracle` rather than trusted either way.
    /// Read-only with respect to `self.state` -- never mutates game state,
    /// only this struct's own oracle bookkeeping fields.
    fn check_discard_phase_oracle(&mut self, actor: u8, line: &Line) {
        let Some(&journal_excess) = self.discard_phase_oracle.get(&(actor, line.round.to_string())) else {
            return;
        };
        let p = &self.state.players[actor as usize];
        let s = effects::state_stats(&self.state, p);
        let limit = s.military_actions + s.military_hand_limit;
        let hand_len = p.hand_military.len();
        let reconstructed_excess = (hand_len as i32 - limit).max(0) as u32;
        self.discard_oracle_checked += 1;
        if reconstructed_excess == journal_excess {
            self.discard_oracle_agreed += 1;
            return;
        }
        if self.discard_oracle_divergence.is_none() {
            let checkpoint = self.military_hand_ledger.get(&(actor, line.round.to_string())).copied();
            let ledger_excess = checkpoint.map(|c| (c.raw - limit).max(0) as u32);
            let ledger_last_event = checkpoint.and_then(|c| c.last_event);
            self.hand_ledger_verdict = Some(match ledger_excess {
                None => HandLedgerVerdict::NoLedgerEntry,
                Some(le) if le == journal_excess => HandLedgerVerdict::SimulatorBug,
                Some(_) => HandLedgerVerdict::UnmodelledEvent(ledger_last_event.map(|(kind, _)| kind)),
            });
            self.discard_oracle_divergence = Some(DiscardOracleDivergence {
                lineno: line.lineno,
                round: line.round.to_string(),
                age: line.age.to_string(),
                actor: Color::parse(line.color).map(Color::as_str).unwrap_or("?"),
                journal_excess,
                reconstructed_excess,
                hand_len,
                limit,
                ledger_excess: ledger_excess.unwrap_or(0),
                ledger_last_event,
            });
        }
    }

    /// The actual culture-oracle compare-and-record step, shared by the
    /// immediate path (an ordinary `EndTurn` whose production already ran)
    /// and [`Self::flush_pending_culture_check`] (a deferred one). See
    /// [`CultureOracleDivergence`]'s own doc.
    ///
    /// Reads `state.last_end_of_turn_culture[actor_seat]`, NOT the live
    /// `state.players[actor_seat].culture` -- see that field's own doc.
    /// Whichever path calls this, `game::resume_end_turn` has, by
    /// construction, ALREADY run (the immediate path just finished its own
    /// `try_apply(Move::EndTurn, ..)`; the deferred path only gets here once
    /// `flush_pending_culture_check` has confirmed the actor's own
    /// `DiscardMilitary` pending is gone), so the snapshot for THIS actor's
    /// THIS turn is always fresh -- `resume_end_turn` sets it unconditionally,
    /// synchronously, before it can call `advance_turn`. Consumed (reset to
    /// `None`) on every read so a stale value from several turns ago can
    /// never silently stand in for a checkpoint it does not belong to.
    fn record_culture_check(&mut self, lineno: usize, actor_seat: u8, journal_now: i32, last_action_class: Option<ActionClass>) {
        let got = self.state.last_end_of_turn_culture[actor_seat as usize]
            .take()
            .unwrap_or_else(|| {
                panic!(
                    "record_culture_check called for actor {actor_seat} at line {lineno} but \
                     resume_end_turn never snapshotted this turn's post-production culture -- \
                     a caller reached this checkpoint before economy::end_of_turn actually ran"
                )
            }) as i32;
        self.culture_oracle_checked += 1;
        if journal_now == got {
            self.culture_oracle_agreed += 1;
            return;
        }
        if crate::debugflags::replay_debug() {
            eprintln!(
                "DEBUG end-turn culture drift: actor={actor_seat} journal says (now {journal_now}), this binary \
                 computes {got} (delta {}) at line {lineno}",
                got - journal_now,
            );
        }
        if self.culture_oracle_divergence.is_none() {
            self.culture_oracle_divergence = Some(CultureOracleDivergence {
                lineno,
                actor: Color::from_seat(actor_seat).map(Color::as_str).unwrap_or("?"),
                journal_now,
                reconstructed: got,
                last_action_class,
            });
        }
    }

    /// Flushes a [`PendingCultureCheck`] left by a prior `EndTurn` line
    /// whose own production was blocked on an open discard decision -- see
    /// that struct's own doc for why comparing immediately there would be a
    /// false positive. Called unconditionally at the top of every line's own
    /// dispatch (`replay_game`'s main loop): SELF-DEFERRING, not merely
    /// one-shot -- if `actor`'s `DiscardMilitary` pending is still open (the
    /// resolving `"<Color> discards N card(s)"` line has not been reached
    /// yet), puts the same check straight back and tries again next line,
    /// rather than guessing how many lines the resolution takes. A no-op
    /// when there is nothing pending at all -- the common case, every
    /// ordinary `EndTurn` never touches this field.
    fn flush_pending_culture_check(&mut self) {
        let Some(pending) = self.pending_culture_check.take() else { return };
        if matches!(self.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary && c.player == pending.actor_seat)
        {
            self.pending_culture_check = Some(pending); // still blocked -- try again next line
            return;
        }
        self.record_culture_check(pending.lineno, pending.actor_seat, pending.journal_now, pending.last_action_class);
    }

    /// [`Replayer::record_culture_check`]'s science twin -- see
    /// [`ScienceOracleDivergence`]'s own doc. Only ever called when
    /// `debugflags::science_oracle()` is set (the dispatch loop's own gate),
    /// so `science_oracle_checked`/`_agreed`/`_divergence` stay at their
    /// `Replayer::new` defaults for every ordinary run.
    fn record_science_check(&mut self, lineno: usize, round: &str, actor_seat: u8, journal_now: i32, last_action_class: Option<ActionClass>) {
        let got = self.state.last_end_of_turn_science[actor_seat as usize]
            .take()
            .unwrap_or_else(|| {
                panic!(
                    "record_science_check called for actor {actor_seat} at line {lineno} but \
                     resume_end_turn never snapshotted this turn's post-production science -- \
                     a caller reached this checkpoint before economy::end_of_turn actually ran"
                )
            }) as i32;
        self.science_oracle_checked += 1;
        if journal_now == got {
            self.science_oracle_agreed += 1;
            return;
        }
        if crate::debugflags::replay_debug() {
            eprintln!(
                "DEBUG end-turn science drift (SCIENCE_ORACLE): actor={actor_seat} journal says (now {journal_now}), \
                 this binary computes {got} (delta {}) at line {lineno}",
                got - journal_now,
            );
        }
        if self.science_oracle_divergence.is_none() {
            self.science_oracle_divergence = Some(ScienceOracleDivergence {
                lineno,
                round: round.to_string(),
                actor: Color::from_seat(actor_seat).map(Color::as_str).unwrap_or("?"),
                journal_now,
                reconstructed: got,
                last_action_class,
            });
        }
    }

    /// [`Replayer::flush_pending_culture_check`]'s science twin.
    fn flush_pending_science_check(&mut self) {
        let Some(pending) = self.pending_science_check.take() else { return };
        if matches!(self.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary && c.player == pending.actor_seat)
        {
            self.pending_science_check = Some(pending); // still blocked -- try again next line
            return;
        }
        self.record_science_check(pending.lineno, &pending.round, pending.actor_seat, pending.journal_now, pending.last_action_class);
    }

    /// [`Replayer::record_culture_check`]'s resources twin -- see
    /// [`ResourceOracleDivergence`]'s own doc. `RESOURCE_ORACLE`-gated, same
    /// never-called-unless-flagged guarantee as `record_science_check`.
    fn record_resource_check(&mut self, lineno: usize, round: &str, actor_seat: u8, journal_now: i32, last_action_class: Option<ActionClass>) {
        let got = self.state.last_end_of_turn_resources[actor_seat as usize]
            .take()
            .unwrap_or_else(|| {
                panic!(
                    "record_resource_check called for actor {actor_seat} at line {lineno} but \
                     resume_end_turn never snapshotted this turn's post-production resources -- \
                     a caller reached this checkpoint before economy::end_of_turn actually ran"
                )
            }) as i32;
        self.resource_oracle_checked += 1;
        if journal_now == got {
            self.resource_oracle_agreed += 1;
            return;
        }
        if crate::debugflags::replay_debug() {
            eprintln!(
                "DEBUG end-turn resources drift (RESOURCE_ORACLE): actor={actor_seat} journal says (now {journal_now}), \
                 this binary computes {got} (delta {}) at line {lineno}",
                got - journal_now,
            );
        }
        if self.resource_oracle_divergence.is_none() {
            self.resource_oracle_divergence = Some(ResourceOracleDivergence {
                lineno,
                round: round.to_string(),
                actor: Color::from_seat(actor_seat).map(Color::as_str).unwrap_or("?"),
                journal_now,
                reconstructed: got,
                last_action_class,
            });
        }
    }

    /// [`Replayer::flush_pending_culture_check`]'s resources twin.
    fn flush_pending_resource_check(&mut self) {
        let Some(pending) = self.pending_resource_check.take() else { return };
        if matches!(self.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary && c.player == pending.actor_seat)
        {
            self.pending_resource_check = Some(pending); // still blocked -- try again next line
            return;
        }
        self.record_resource_check(pending.lineno, &pending.round, pending.actor_seat, pending.journal_now, pending.last_action_class);
    }

    /// Flushes every [`Replayer::pending_culture_corrections`] entry whose
    /// stated `old` NOW matches this reconstruction's live culture for that
    /// player -- see that field's own doc for why a `"GAME DATA UPDATED"`
    /// clause can't always apply the instant its own line is read (a war's
    /// spoils, deferred on our side until `game::start_turn`'s cascade,
    /// hadn't landed yet). Called unconditionally at the top of every
    /// line's own dispatch, same SELF-DEFERRING convention as `flush_
    /// pending_culture_check` just above: entries that still don't match are
    /// put straight back, tried again next line, for as long as the game
    /// runs. A no-op the overwhelming majority of the time -- this field is
    /// empty for every game but the two sampled corpus journals that carry
    /// a `"GAME DATA UPDATED ... culture: ..."` line at all.
    fn flush_pending_culture_corrections(&mut self) {
        if self.pending_culture_corrections.is_empty() {
            return;
        }
        let still_pending: Vec<(Color, i32, i32)> = self
            .pending_culture_corrections
            .drain(..)
            .filter(|&(color, old, new)| {
                let p = &mut self.state.players[color.seat() as usize];
                if p.culture as i32 == old {
                    p.culture = new.max(0) as u16;
                    false // applied -- drop from the pending list
                } else {
                    true // still doesn't match -- keep trying
                }
            })
            .collect();
        self.pending_culture_corrections = still_pending;
    }

    /// The resources twin of [`Replayer::flush_pending_culture_corrections`]
    /// -- same self-deferring retry, `p.resources` in place of `p.culture`.
    /// See [`Replayer::pending_resource_corrections`]'s own doc for why this
    /// exists at all.
    ///
    /// Unlike culture (a plain scalar with no blue-token backing), a
    /// `resources` correction MUST go through [`economy::gain_resources`]/
    /// [`economy::pay_resources`], not a bare `p.resources = new`: those are
    /// the only chokepoints that also update `p.resource_tokens` (the
    /// per-denomination placement history -- `crate::state::TokenBank`'s own
    /// doc). A bare scalar overwrite here is exactly the same class of bug
    /// TOKENCOUNT's own pass fixed in the `food_and_or_resources` correction
    /// a few hundred lines up: `economy::blue_used`'s reconciliation either
    /// silently drops a downward correction's excess (leaving the bank
    /// permanently holding more tokens than the corrected total justifies)
    /// or freshly re-packs an upward one on top of an already-stale bank.
    fn flush_pending_resource_corrections(&mut self) {
        if self.pending_resource_corrections.is_empty() {
            return;
        }
        let still_pending: Vec<(Color, i32, i32)> = self
            .pending_resource_corrections
            .drain(..)
            .filter(|&(color, old, new)| {
                let p = &mut self.state.players[color.seat() as usize];
                if p.resources as i32 == old {
                    let new = new.max(0);
                    let delta = new - old;
                    if delta > 0 {
                        economy::gain_resources(p, delta as u16);
                    } else if delta < 0 {
                        economy::pay_resources(p, (-delta) as u16);
                    }
                    false // applied -- drop from the pending list
                } else {
                    true // still doesn't match -- keep trying
                }
            })
            .collect();
        self.pending_resource_corrections = still_pending;
    }

    /// The science twin of [`Replayer::flush_pending_culture_corrections`]
    /// -- same self-deferring retry, `p.science` in place of `p.culture`.
    /// See [`Replayer::pending_science_corrections`]'s own doc for why this
    /// exists at all. Science, like culture, is a plain scalar with no
    /// token-bank involvement, so this mirrors the culture flush exactly
    /// (a bare `p.science = new.max(0) as u16`), NOT the resources one.
    fn flush_pending_science_corrections(&mut self) {
        if self.pending_science_corrections.is_empty() {
            return;
        }
        let still_pending: Vec<(Color, i32, i32)> = self
            .pending_science_corrections
            .drain(..)
            .filter(|&(color, old, new)| {
                let p = &mut self.state.players[color.seat() as usize];
                if p.science as i32 == old {
                    p.science = new.max(0) as u16;
                    false // applied -- drop from the pending list
                } else {
                    true // still doesn't match -- keep trying
                }
            })
            .collect();
        self.pending_science_corrections = still_pending;
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
                other @ ChoiceOption::Slot(_) | other @ ChoiceOption::Move(_) | other @ ChoiceOption::Gain(_) | other @ ChoiceOption::Word(_) => panic!("DiscardMilitary choice offered a non-card option {other:?}"),
            })
            .collect();
        let (n, certainty) = self.discard_solver.choose(c.player, self.current_lineno, &opts);
        if crate::debugflags::replay_debug_all() {
            eprintln!(
                "DEBUG discard: player {} line {} picked {} of {} candidates ({certainty:?})",
                c.player,
                self.current_lineno,
                opts[n].get().name,
                opts.len()
            );
        }
        self.apply_move(Move::Choose { n: n as u8 });
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
            if crate::debugflags::replay_debug() {
                let p = &self.state.players[self.state.current as usize];
                eprintln!(
                    "DEBUG try_apply fail: mv={mv:?} actor(current)={} civil_actions={} military_actions={} government={} leader={} phase={:?} pending_top={:?} hand_civil_size={} civil_hand_limit={} hand_civil={:?} resources={} food={} science={} mil_discount={} workers_free={} one_time_discount={:?} tableau={:?} card_row={:?}",
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
                    p.science,
                    p.mil_discount,
                    p.workers_free,
                    p.one_time_discount,
                    p.techs.iter().map(|(t, slot)| format!("{}x{}", t.get().name, slot.workers)).collect::<Vec<_>>(),
                    (0..13).map(|i| if self.state.card_row[i].is_none() { "-".to_string() } else { self.state.card_row[i].get().name.to_string() }).collect::<Vec<_>>(),
                );
                // Develop/PlayAction failures are usually a card-identity or
                // affordability question specifically -- surface the exact
                // CardId this binary attempted, whether it's really in
                // `hand_civil` (a wrong age sibling would fail this even
                // though a same-named card IS present above), and this
                // binary's own computed science price for it.
                match mv {
                    Move::Develop { card } | Move::PlayAction { card } => {
                        eprintln!(
                            "DEBUG develop/play detail: card={:?} age={:?} in_hand_civil={} tech_cost_net={:?}",
                            card,
                            card.get().age,
                            p.hand_civil.as_slice().contains(&card),
                            costs::tech_cost_net(&self.state, p, card),
                        );
                    }
                    Move::Take { .. } | Move::Build { .. } | Move::Upgrade { .. } | Move::WonderStep { .. } | Move::Pop | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => {}
                }
                if let Move::Build { card } = mv {
                    eprintln!(
                        "DEBUG cost detail: build_cost_for({:?})={:?}",
                        card.get().name,
                        costs::build_cost_for(&self.state, p, card),
                    );
                }
                if let Move::Upgrade { from, to } = mv {
                    eprintln!(
                        "DEBUG cost detail: upgrade_cost({:?}->{:?})={} (lo={:?} hi={:?})",
                        from.get().name,
                        to.get().name,
                        costs::upgrade_cost(&self.state, p, from, to),
                        costs::build_cost_for(&self.state, p, from),
                        costs::build_cost_for(&self.state, p, to),
                    );
                }
                if let Move::WonderStep { steps } = mv {
                    // `costs::wonder_stage_cost` itself `debug_assert!`s
                    // `!p.wonder.is_none()` (a real precondition -- its
                    // whole `stages[done..end]` slice is meaningless with
                    // no wonder in play) -- calling it unconditionally here
                    // to print a diagnostic for an attempted `WonderStep`
                    // with NO wonder in progress (the exact illegal move
                    // this whole branch exists to describe) tripped that
                    // assert and aborted the entire `replaystats` process
                    // partway through the corpus (found while measuring the
                    // civil-action-total question, `docs/REPLAY.md` "civil
                    // action total" handoff, games `7522899`/`7521762` --
                    // NOT that pass's own WonderStep-bucket bug to fix,
                    // this is a debug-print robustness gap in the
                    // diagnostic itself). Guarded the same way the next
                    // line already guards its OWN `p.wonder.get().name`.
                    eprintln!(
                        "DEBUG cost detail: wonder_stage_cost(steps={steps})={} wonder={} wonder_steps={}",
                        if p.wonder.is_none() { -1 } else { costs::wonder_stage_cost(&self.state, p, steps) },
                        if p.wonder.is_none() { "none" } else { p.wonder.get().name },
                        p.wonder_steps,
                    );
                }
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
        let actor = self.state.current;
        let pending_top_before = self.state.pending.top().cloned();
        let hand_military_len_before: [usize; MAX_PLAYERS] =
            std::array::from_fn(|i| self.state.players[i].hand_military.len());
        self.apply_move(mv);
        for seat in 0..self.state.num_players {
            let before = hand_military_len_before[seat as usize];
            let after = self.state.players[seat as usize].hand_military.len();
            if after > before {
                self.repay_military_hand_deficit(seat, (after - before) as u32);
            }
        }
        if crate::debugflags::replay_debug_all() {
            let p = &self.state.players[self.state.current as usize];
            eprintln!(
                "DEBUG applied mv={mv:?} -> current={} civil_actions={} military_actions={} phase={:?} round={} pending_before={:?} yellow_bank={} food={} resources={}",
                self.state.current, p.civil_actions, p.military_actions, self.state.phase, self.state.round, pending_top_before, p.yellow_bank, p.food, p.resources
            );
            if matches!(mv, Move::EndTurn) {
                // The actor whose turn just ended -- `self.state.current` has
                // already advanced to the NEXT player by this point, so this
                // must be indexed by the pre-apply `actor`, not `.current`.
                let ap = &self.state.players[actor as usize];
                eprintln!(
                    "DEBUG end_turn totals: actor={actor} resources={} food={} science={} culture={}",
                    ap.resources, ap.food, ap.science, ap.culture
                );
            }
        }
        Ok(())
    }

    /// Applies a journal-observed `Move::Take { slot }` exactly like
    /// [`Self::try_apply`], EXCEPT: if the engine's own `legal::legal_moves`
    /// rejects it, and [`take_blocked_only_by_hand_full`] confirms
    /// `costs::take_gate`'s `hand_full` gate is the ONLY reason (every other
    /// `costs::take_rejection` gate agrees it would otherwise be legal),
    /// this accepts it instead of raising `IllegalMove: Take` -- see that
    /// function's own doc and `docs/REPLAY.md`'s Take/HandFull "genuinely
    /// unexplained discrepancy" conclusion for why. `costs::take_gate` and
    /// `legal::legal_moves` are only ever CONSULTED here, never modified --
    /// self-play legality is untouched by this method existing.
    ///
    /// If the move is illegal for ANY other reason too (or legal outright),
    /// this defers entirely to `try_apply`, so every other `IllegalMove:
    /// Take` mismatch this file already produces is unaffected.
    fn try_apply_take(&mut self, actor: u8, slot: u8) -> Result<(), MismatchKind> {
        let mv = Move::Take { slot };
        let legal = legal::legal_moves(&self.state);
        if !legal.as_slice().contains(&mv)
            && take_blocked_only_by_hand_full(&self.state, &self.state.players[actor as usize], slot as usize)
        {
            self.hand_full_takes_overridden += 1;
            // Same decision-recording `try_apply`'s own `record=true` path
            // does, using the ENGINE's own (necessarily hand_full-excluding)
            // `legal_moves` list. `humandata.rs` already handles a
            // `human_move` absent from `legal_moves` by skipping that one
            // data point ("human_move not found in legal_moves ...,
            // skipping") -- the correct degradation for a move this file
            // KNOWINGLY accepts despite the engine calling it illegal, not a
            // new failure mode.
            if self.record_decisions {
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
            self.apply_move(mv);
            return Ok(());
        }
        self.try_apply(mv, true)
    }
}

/// REPLAYER-ONLY divergence from self-play legality (`docs/REPLAY.md`'s
/// Take/HandFull handoffs): whether a journal-observed take of `slot` is
/// illegal ONLY because `costs::take_gate`'s `hand_full` gate rejects it --
/// settled correct by primary source (Code of Laws, verbatim: "The number
/// of civil cards in your hand is limited by your civil action total. When
/// you are at or above the limit, you may not add another civil card to
/// your hand by any means.") and left untouched here -- with every OTHER
/// `costs::take_rejection` gate (cost, duplicate-name, one-leader-per-age,
/// wonder rules) agreeing the take would otherwise be legal.
///
/// Discriminated using `costs::take_rejection` itself, called TWICE, never
/// a bespoke reimplementation of its gate logic: once with the real gate
/// (must name `HandFull` specifically -- any other named reason is an
/// honest mismatch this function must not touch), once more with a copy
/// whose `hand_full` is forced `false` (must then return `None`, proving no
/// OTHER gate also blocks this exact take). `costs::take_gate`/`legal.rs`
/// are read, never modified, by this function or its one call site
/// ([`Replayer::try_apply_take`]) -- self-play legality is unaffected.
fn take_blocked_only_by_hand_full(state: &GameState, p: &PlayerState, slot: usize) -> bool {
    let gate = costs::take_gate(state, p, None);
    if !matches!(costs::take_rejection(state, p, slot, &gate), Some(costs::TakeRejection::HandFull)) {
        return false;
    }
    let probe = costs::TakeGate { hand_full: false, ..gate };
    costs::take_rejection(state, p, slot, &probe).is_none()
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
            // Civil Life's CA-free exemption: the player needs NO civil
            // action to exercise this discount, so the only gate is
            // resource affordability (food). `legal.rs` checks this the
            // same way: `ca >= 1 || civil_life_ca_free(pop_food)`.
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
            // CA-free exemption: no CA needed, only resource check.
            (p.resources as i32 >= cost).then_some(Move::Build { card })
        }
        ActionClass::DevelopTechnology if p.one_time_discount.develop_science != 0 => {
            let card = card?;
            if !p.hand_civil.contains(card) {
                return None;
            }
            let cost = costs::tech_cost(&r.state, p, card)?;
            // CA-free exemption: no CA needed, only science check.
            (p.science as i32 >= cost).then_some(Move::Develop { card })
        }
        ActionClass::TakeCard | ActionClass::BuildBuilding | ActionClass::BuildUnit | ActionClass::BuildWonderStage | ActionClass::IncreasePopulation | ActionClass::UpgradeUnit | ActionClass::UpgradeProduction | ActionClass::DevelopTechnology | ActionClass::ElectLeader | ActionClass::ChangeGovernment | ActionClass::PlayTactic | ActionClass::DeclareWar | ActionClass::WinWar | ActionClass::PlayAggression | ActionClass::ProposePact | ActionClass::AcceptPact | ActionClass::Colonize | ActionClass::Discard | ActionClass::Bid | ActionClass::WinAuction | ActionClass::Destroy | ActionClass::Disband | ActionClass::Pass | ActionClass::PlayEvent | ActionClass::PlayActionCard | ActionClass::PutBack | ActionClass::EndTurn | ActionClass::RemoveLeaderYellow | ActionClass::ColumbusColonize | ActionClass::Barbarossa | ActionClass::BachTheater => None,
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

/// The banked-science RUNNING TOTAL BGO prints in an `"End turn <Color>
/// scores: ...; N science (now M); ..."` line -- `M`, not `N` (`N` is the
/// per-turn production rate the line labels "science", `M` in the trailing
/// `"(now M)"` is the authoritative post-turn total after every gain/spend
/// this whole turn, including event/leader-ability clauses the rate alone
/// never reflects -- confirmed against the raw corpus). Investigation-only
/// helper (`REPLAY_DEBUG`'s end-turn science drift check) for chasing
/// science-payment mismatches upstream of the actual spend -- see the
/// `IllegalMove: Develop`/`PlayAction` buckets this exists for.
fn trailing_now_science(text: &str) -> Option<i32> {
    let p = text.find(" science (now ")?;
    let rest = &text[p + " science (now ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    rest[..digits_end].parse().ok()
}

/// The banked-resources RUNNING TOTAL BGO prints in an `"End turn <Color>
/// scores: ...; N resources (now M); ..."` line -- `M`, the authoritative
/// post-turn total, mirrors [`trailing_now_science`] exactly but for the
/// `IllegalMove: Build` bucket's own "resources < build_cost_for(...)"
/// investigation (2026-08-14 session): a resource shortfall discovered only
/// at the eventual `Build` spend, many lines downstream of whatever actually
/// under-counted it, is the same "check the oracle every turn, not just at
/// the spend" shape `trailing_now_science` already solved for science.
/// Investigation-only, `REPLAY_DEBUG`-gated, same discard-blocked-turn
/// caveat as `trailing_now_science`'s own doc comment.
fn trailing_now_resources(text: &str) -> Option<i32> {
    let p = text.find(" resources (now ")?;
    let rest = &text[p + " resources (now ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    rest[..digits_end].parse().ok()
}

/// Applies Winston Churchill's once-per-turn choice
/// (`sources/bga_throughtheages_material.inc.php` #186: "On your turn,
/// choose one: score 3 culture; or you have 3 science and 3 resources for
/// developing military unit technologies and building and upgrading
/// units.") when `end_turn_text` -- the SAME no-leading-colour "End turn"
/// line `replay_game`'s own dispatch loop is about to translate into
/// `Move::EndTurn` (the loop's own actor, already resolved as `r.state.
/// current`, is who this move belongs to) -- carries it as a glued-on
/// PREFIX consequence clause: `"End turn Winston Churchill scores 3
/// culture.; <Color> scores: ..."`. BGO never logs this as a separate
/// line, so nothing else in this file ever sees it; a plain `Ok(())` no-op
/// for every other "End turn" line.
///
/// Confirmed corpus-wide (every `sources/bgo` journal): 1,049 occurrences
/// across 377 games, EVERY one this exact culture phrasing -- the card's
/// "3 science and 3 resources" military option is never once observed in
/// the corpus, so this only ever reads for `ChurchillChoice::Culture`.
/// Applying the move here, BEFORE the caller's own `Move::EndTurn`, mirrors
/// the real turn order: Churchill's choice is an Action-phase move, always
/// exercised before the turn ends. `try_apply` legality-checks it the same
/// as every other synthesized move this file inserts
/// (`RemoveLeaderYellow`, `Barbarossa`, `ColumbusColonize`) -- a game that
/// reaches here without Churchill actually in play, or with this turn's
/// choice already spent, fails loud here rather than silently drifting
/// culture for the rest of the game.
///
/// Before this existed, the replayer never modelled this choice at all --
/// the +3 culture (or the ring-fenced military resources, never observed)
/// was silently dropped every single turn a Churchill owner had him in
/// play, undercounting `state.players[_].culture` for the rest of the
/// game and, downstream, the final score `docs/REPLAY.md`'s own "Final
/// scores" section already tracks at length.
fn apply_churchill_end_turn_choice(r: &mut Replayer, end_turn_text: &str) -> Result<(), MismatchKind> {
    if end_turn_text.starts_with("End turn Winston Churchill scores 3 culture.") {
        r.try_apply(Move::Churchill { choice: ChurchillChoice::Culture }, true)?;
    }
    Ok(())
}

/// [`apply_churchill_end_turn_choice`]'s MILITARY twin. That function's own
/// doc comment claimed the "3 science and 3 resources for developing
/// military unit technologies and building/upgrading units" option was
/// "never once observed in the corpus" -- WRONG: BGO simply never announces
/// this choice as its own line (there is no culture-style "scores" clause
/// to glue it onto), so the only observable evidence is downstream, on a
/// LATER Build/Upgrade/Develop line's `"loses N military resource"`/`"loses
/// N military science"` clause (`lost_military_resource`/
/// `lost_military_science`'s own doc comments -- one of them already flagged
/// this exact shape as "a currently-unexplained baseline case" without
/// chasing it: lines showing the clause with NO preceding Wave of
/// Nationalism/Military Build-Up/Patriotism grant visible in the journal at
/// all). Confirmed against the corpus's two largest `IllegalMove` buckets
/// (`Build`/`Develop`): 24 and 26 respectively of their failures are a unit
/// build or a unit-technology develop by a Winston-Churchill-led player
/// whose `p.mil_discount`/`p.mil_sci_discount` this binary had never once
/// been given a chance to fill, because nothing in this file ever applied
/// `Move::Churchill{Military}` at all before this fix.
///
/// Called right before a Build/Upgrade/Develop line is applied (mirroring
/// where `apply_churchill_end_turn_choice` sits relative to `Move::EndTurn`
/// -- Churchill's choice is always an Action-phase move exercised BEFORE
/// whatever it pays for). Gated on the CURRENT pool actually being short of
/// what the line claims was drawn from it, not merely on the clause's
/// presence -- a player who already topped the pool up from Wave of
/// Nationalism/Military Build-Up/Patriotism this turn needs no synthesis,
/// and synthesizing anyway would incorrectly mark `churchill_used`, breaking
/// this turn's OWN end-of-turn Culture-choice line (mutually exclusive by
/// the rules: `try_apply` legality-checks this exactly like every other
/// synthesized move this file inserts, so a wrong guess fails loud here
/// rather than silently drifting the pool).
fn maybe_synthesize_churchill_military(r: &mut Replayer, actor: u8, raw_text: &str) -> Result<(), MismatchKind> {
    let need_resource = lost_military_resource(raw_text).unwrap_or(0);
    let need_science = lost_military_science(raw_text).unwrap_or(0);
    if need_resource == 0 && need_science == 0 {
        return Ok(());
    }
    let p = &r.state.players[actor as usize];
    let short = need_resource > p.mil_discount as i32 || need_science > p.mil_sci_discount as i32;
    if !short || p.leader.is_none() || p.leader.get().name != "Winston Churchill" || p.churchill_used {
        return Ok(());
    }
    r.try_apply(Move::Churchill { choice: ChurchillChoice::Military }, true)
}

/// DIAGNOSTIC ONLY (temporary, this pass): [`trailing_now_science`]'s twin
/// for the `"N culture (now M)"` clause of an `"End turn"` line, used to
/// bisect where a completed game's running culture total first drifts from
/// BGO's own recorded running total, ahead of `finish_game`.
fn trailing_now_culture(text: &str) -> Option<i32> {
    let p = text.find(" culture (now ")?;
    let rest = &text[p + " culture (now ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    rest[..digits_end].parse().ok()
}

/// [`spent_resources`]'s twin for the food clause of a `"<Color> increases
/// population <Color> spends N food"` line. An EARLIER version of this
/// doc comment claimed food is "the ONLY clause a Pop line ever carries" --
/// FALSE, found chasing the `IllegalMove: Pop` bucket (`docs/REPLAY.md`):
/// a live Trade Routes Agreement grant (§5.9) lets a Pop be paid PART in
/// converted resources, and BGO prints that as a SECOND clause on the SAME
/// line -- `"<Color> increases population <Color> spends N food; <Color>
/// spends M resource"` -- not folded into the food number the way an
/// earlier pass assumed. This function still reads only the food clause;
/// see [`spent_resource_after_food`] for the optional second one.
fn spent_food(text: &str) -> Option<i32> {
    let p = text.find(" spends ")?;
    let rest = &text[p + " spends ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    if !rest[digits_end..].starts_with(" food") {
        return None; // "spends N resource" etc. -- not this check's business
    }
    rest[..digits_end].parse().ok()
}

/// The optional SECOND `"; <Color> spends M resource(s)"` clause that
/// follows a Pop line's own `"<Color> spends N food"` clause -- see
/// [`spent_food`]'s doc comment for why this exists and the real corpus
/// shape it reads (thousands of occurrences, e.g. game `7522658` line 289:
/// `"Purple increases population Purple spends 2 food; Purple spends 1
/// resource"`). Searches from the SECOND `" spends "` in `text` (the
/// first is always the food clause `spent_food` reads), not the first, so
/// this never re-reads the food clause's own number as if it were a
/// resource amount. Returns `0`, not `None`, when there is no second
/// clause -- every caller wants a plain amount to add to the food figure,
/// not an `Option` to unwrap.
fn spent_resource_after_food(text: &str) -> i32 {
    let Some(first) = text.find(" spends ") else { return 0 };
    let after_first = first + " spends ".len();
    let Some(second_rel) = text[after_first..].find(" spends ") else { return 0 };
    let rest = &text[after_first + second_rel + " spends ".len()..];
    let digits_end = match rest.find(|c: char| !c.is_ascii_digit()) {
        Some(0) | None => return 0,
        Some(n) => n,
    };
    if !rest[digits_end..].starts_with(" resource") {
        return 0;
    }
    rest[..digits_end].parse().unwrap_or(0)
}

/// The number right after `" loses "` in `text`, if the SAME clause names
/// `"military resource"` -- e.g. `"Purple builds Warrior Purple loses 1
/// military resource; Purple spends 1 resource"`. BGO's UI splits a unit
/// build/upgrade's total resource payment into two clauses by SOURCE: any
/// portion covered by `p.mil_discount` (Patriotism/Wave of Nationalism/
/// Military Build-Up's "pay N fewer resources [for military units]" pool,
/// OR Winston Churchill's own once-per-turn "3 resources for military
/// units" choice, `costs::spend_mil_discount`) is printed as `"loses N
/// military resource"`, and any REMAINING portion paid from the ordinary
/// resource pool as `"spends N resource"` -- found by replaying real BGO
/// games (`docs/REPLAY.md` fifth pass): `loses` + `spends` summed always
/// equals exactly this binary's own `costs::build_cost_for` for the unit,
/// including lines with NO preceding Patriotism-style grant visible in the
/// journal at all -- previously an unexplained baseline case, now
/// understood to be Churchill's silent choice: BGO never announces it as
/// its own line, so this clause IS the only observable evidence
/// (`maybe_synthesize_churchill_military`'s own doc comment). Reading
/// `"spends"` alone (the OLD behaviour) silently under-counted the
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

/// [`lost_military_resource`]'s DEVELOP-side twin: the number right after
/// `" loses "` in `text`, if the SAME clause names `"military science"` --
/// Winston Churchill's military choice's other pool (`p.mil_sci_discount`,
/// [`costs::spend_mil_sci_discount`]), e.g. `"Orange discovers Air Forces
/// Orange loses 3 military science; Orange loses 9 science"`. Anchoring on
/// the FIRST `" loses "` clause (same as `lost_military_resource`) is safe
/// here for the identical reason: BGO always prints the pool-sourced
/// portion first, ahead of the ordinary `"loses N science"` remainder.
fn lost_military_science(text: &str) -> Option<i32> {
    let p = text.find(" loses ")?;
    let rest = &text[p + " loses ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    if !rest[digits_end..].starts_with(" military science") {
        return None; // e.g. "loses 9 science" -- the ordinary remainder, not this clause
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

/// The optional trailing `"; <Color> spends N food"` clause on a
/// build/upgrade/wonder-stage line, whatever precedes it (`"spends M
/// resource(s)"`, `"loses M military resource"`, or both) -- Trade Routes
/// Agreement side A's "1 food as 1 resource" grant (§5.9) folded into the
/// SAME printed line, the mirror of [`spent_resource_after_food`]'s Pop-line
/// shape in the OTHER direction. Found chasing the `UnrecoverableHiddenInfo:
/// build cost mismatch` bucket (`docs/REPLAY.md`): real corpus games pay
/// PART of a build/upgrade/wonder-stage's resource cost in converted food
/// (e.g. game `7523070` line 143, `"Green builds Warrior Green spends 1
/// resource; Green spends 1 food"` for a 2-resource Warrior, confirmed by
/// full-game reconciliation that Green's resources were untouched -- the
/// `"1 food"` is a real second payment, not a rendering quirk), and this
/// binary's [`total_paid_for_build`] previously read only the FIRST clause,
/// silently under-counting the true total by exactly the food amount.
/// Searches from the LAST `" spends "` in `text`, not the first (so a
/// resource-only line's own single clause is never misread as this one) --
/// a build/upgrade/wonder-stage line never carries a food clause for any
/// OTHER reason, unlike a Pop line's own food-native cost. Returns `0`, not
/// `None`: every caller wants a plain amount to add to the resource figure,
/// not an `Option` to unwrap.
fn spent_food_after_resource(text: &str) -> i32 {
    let Some(p) = text.rfind(" spends ") else { return 0 };
    let rest = &text[p + " spends ".len()..];
    let digits_end = match rest.find(|c: char| !c.is_ascii_digit()) {
        Some(0) | None => return 0,
        Some(n) => n,
    };
    if !rest[digits_end..].starts_with(" food") {
        return 0;
    }
    rest[..digits_end].parse().unwrap_or(0)
}

/// The TOTAL a build/upgrade/wonder-stage line states it paid, INCLUDING a
/// Trade-Routes-Agreement food clause -- [`total_paid_for_build`]'s own
/// resource-only total plus [`spent_food_after_resource`]'s optional food
/// clause, but unlike a bare `.map()` composition of the two, still returns
/// a real total when the WHOLE payment was converted food and no `"spends N
/// resources"`/`"loses N military resource"` clause appears on the line at
/// all.
///
/// ENGINE BUG this fixes (found chasing the `IllegalMove: Build` bucket,
/// 2026-08-14 session, game `7522518` round 4: `"Orange builds 1 stage of
/// Pyramids Orange spends 1 food; ; Wonder completed"`, Orange holding an
/// accepted Trade Routes Agreement as side A): [`total_paid_for_build`]
/// returns `None` for this line (correctly -- it only ever reads the
/// resource-shaped clauses, per its own doc, and must keep doing so for its
/// OTHER caller, [`resolve_named_card_by_effect`], which solves for a
/// resource DISCOUNT and would be wrongly fed a food-derived number). But
/// every caller further down this file that wants "how much did this line
/// TOTAL" was composing that `None` with `.map(|base| base +
/// spent_food_after_resource(...))`, which stays `None` for an all-food
/// line -- indistinguishable from [`total_paid_for_build`]'s OWN `None`
/// case, a genuinely free build/upgrade/wonder-stage with no clause at all
/// (`total_paid_for_build("Purple builds 1 stage of Pyramids")` == `None`,
/// its own doc-tested case). [`convert_trade_food_shortfall`]'s gate below
/// then silently skipped the whole conversion, leaving the caller's own
/// `Move::Build`/`Move::WonderStep`/`Move::Upgrade` to charge `true_cost`
/// straight out of `p.resources` -- resources the real player never spent,
/// permanently short for the rest of the game (exactly this bucket's
/// dominant "resources short by 1" signature).
fn stated_build_payment(text: &str) -> Option<i32> {
    let food = spent_food_after_resource(text);
    match total_paid_for_build(text) {
        Some(base) => Some(base + food),
        None if food > 0 => Some(food),
        None => None,
    }
}

/// Whether a `build cost mismatch` between the journal's stated total
/// (`want`) and this binary's computed cost (`got`) is explained by a held
/// Masonry / Architecture / Engineering build discount.
///
/// BGO prints the PRE-discount price in a discounted build's "spends"
/// clause: a Drama (Age I urban, printed 4) built with Masonry in play is
/// charged 3 by the engine but printed "spends 4". `costs::build_cost_for`
/// already folds the `build_discount[age]` term into `got`, so when the ONLY
/// difference is that modeled term, `want - got` equals the discount the
/// player holds. The one-time and Shakespeare discounts are NOT in
/// `build_discount`, so they would never reconcile here and still surface as
/// genuine mismatches. Gated on the card being urban, the discount being
/// positive, and the gap matching the held discount EXACTLY -- any other gap
/// is a genuine unmodeled discount source the caller still rejects. Real
/// shape: game `7522941` line 183 (Drama, 4 stated / 3 computed with
/// Masonry) and game `7522309` line 316.
fn build_discount_reconciles(state: &GameState, p: &PlayerState, card: CardId, want: i32, got: i32) -> bool {
    let card_ref = card.get();
    let held_discount = crate::effects::state_stats(state, p).build_discount[card_ref.age as usize];
    card_ref.kind.is_urban() && held_discount > 0 && want - got == held_discount
}

/// Converts whatever shortfall Trade Routes Agreement's side-A "1 food as 1
/// resource" grant (`Move::TradeFoodAsResource`, §5.9) explains between `p`'s
/// CURRENT resources and `true_cost`, as that many `Move::TradeFoodAsResource`
/// moves applied to `r` before the caller's own priced build/upgrade/
/// wonder-stage move -- the build/upgrade/wonder-stage sibling of
/// `ActionClass::IncreasePopulation`'s existing `Move::TradeResourceAsFood`
/// fold, same safety gate, opposite conversion direction (see that arm's own
/// doc comment for why the direction differs: Pop is priced in food, a
/// build/upgrade/wonder-stage is priced in resources).
///
/// Gated on the journal's OWN stated total ([`stated_build_payment`],
/// [`total_paid_for_build`] plus [`spent_food_after_resource`]'s optional
/// food clause, including the all-food case) matching `true_cost` EXACTLY:
/// if they disagree, the real bug is a mispriced cost (a missing discount,
/// drifted resources, ...), not a missing conversion, and converting food
/// here would only mask that bug behind a wrong-for-a-different-reason
/// success (docs/REPLAY.md's Civil Life warning: never loosen a check just
/// to make a mismatch disappear). A caller whose gate does not hold gets
/// `shortfall == 0` and this is a no-op -- every existing failure mode this
/// function does not explain is unchanged, it only ever ADDS a path to
/// success.
fn convert_trade_food_shortfall(r: &mut Replayer, actor: u8, raw_text: &str, true_cost: i32) -> Result<(), MismatchKind> {
    let p = &r.state.players[actor as usize];
    let stated = stated_build_payment(raw_text);
    // Convert exactly the food the journal ITSELF says was spent, not a
    // shortfall derived from what the player could have afforded without the
    // pact. A human may use the conversion even when their plain resources
    // alone already cover the cost -- their choice, e.g. to keep resources
    // back for something else the same turn. Deriving the amount instead
    // drains a real resource that this binary can never recover, and the
    // drain does not fail here: it surfaces as an illegal move several
    // actions later, far from its cause. Confirmed against game 7521765
    // round 5, where the journal reads "spends 2 resources; spends 1 food"
    // with 3 plain resources already on hand.
    let to_convert = match stated {
        Some(stated) if stated == true_cost => spent_food_after_resource(raw_text),
        _ => 0,
    };
    if to_convert > 0
        && to_convert <= crate::economy::trade_food_as_resource_remaining(&r.state, p)
        && to_convert <= p.food as i32
    {
        for _ in 0..to_convert {
            r.try_apply(Move::TradeFoodAsResource, true)?;
        }
    }
    Ok(())
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

/// [`total_action_cost`]'s civil/military clauses kept SEPARATE, rather
/// than summed -- needed by the civil-action-TOTAL undercount check
/// (docs/REPLAY.md "civil action total" handoff) below, which must not
/// conflate the two: a `TakeCard` line occasionally carries BOTH clauses on
/// the SAME line (e.g. `"... uses 1 civil action; ... uses 1 military
/// action"`), and that is NOT a take costing 2 action points combined -- it
/// is Hammurabi's once-per-turn "use one military action as one civil
/// action" conversion (`costs.rs`'s own doc on `hammurabi_conversion_
/// available`) paying the printed civil price out of the MILITARY pool
/// instead. Confirmed against a real game (`7522639`, leader elected
/// `Hammurabi` at line 20, this exact double-clause take at line 68): the
/// naive combined sum overcounts that turn's TRUE civil-pool draw by
/// exactly the converted amount, which is why the first version of this
/// check (using `total_action_cost` directly) manufactured 20 false-
/// positive "undercounts", every single one on a Hammurabi turn, every one
/// off by exactly 1 -- a converted civil action was double-charged, not a
/// gap in `costs::ca_total`.
fn civil_and_military_uses(text: &str) -> (Option<i32>, Option<i32>) {
    let mut civil = None;
    let mut military = None;
    let mut rest = text;
    while let Some(p) = rest.find("uses ") {
        rest = &rest[p + "uses ".len()..];
        let Some(digits_end) = rest.find(|c: char| !c.is_ascii_digit()) else {
            break;
        };
        if digits_end == 0 {
            continue;
        }
        let Ok(n) = rest[..digits_end].parse::<i32>() else {
            break;
        };
        let after = &rest[digits_end..];
        if after.starts_with(" civil action") {
            civil = Some(civil.unwrap_or(0) + n);
        } else if after.starts_with(" military action") {
            military = Some(military.unwrap_or(0) + n);
        }
        rest = after;
    }
    (civil, military)
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
            out.extend(std::iter::repeat_n(one, n as usize));
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

/// [`trailing_produces`] with a fallback for the ONE corpus anomaly (game
/// `7521799` line 107: `"Purple plays Reserves Purple spends 2 resource"`)
/// where BGO logged Reserves' GAIN as a `spends` clause instead of `produces`.
/// Reserves (`Special::GainFoodOrResources`) has no card cost and only ever
/// GAINS food/resources, so a trailing `"spends N resource/food"` on its own
/// line cannot be a real cost -- it must be the gain. The `produces` form is
/// tried first (4,157 of 4,158 "plays Reserves" lines use it), and only when
/// that misses does the `spends` fallback fire, so the 9,090 ordinary
/// `"spends N resource"` cost lines elsewhere are never touched.
fn trailing_reserves_gain(text: &str) -> Option<(bool, i32)> {
    if let Some(v) = trailing_produces(text) {
        return Some(v);
    }
    let p = text.rfind(" spends ")?;
    let rest = &text[p + " spends ".len()..];
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: i32 = rest[..digits_end].parse().ok()?;
    if rest[digits_end..].starts_with(" resource") {
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

/// The `"gets N military resource"` clause anywhere in `text` -- Patriotism's
/// own printed bonus (`"<Color> plays Patriotism <Color> gets N military
/// resource; <Color> gets 1 military action"`), which is the ONLY
/// disambiguating evidence for WHICH of its four age-siblings (A/I/II/III,
/// `resourcesForMilitaryUnits` 1/2/3/4) was actually played -- `card_index`
/// resolves a bare `"takes Patriotism in hand"` line age-blind
/// (`best_age_sibling`'s doc comment: "highest age not exceeding the
/// current one"), which is simply WRONG whenever the row/deck actually dealt
/// an OLDER-age copy than that guess; confirmed against a real 2p game
/// (`7521776`): the guessed Age I copy (`resourcesForMilitaryUnits: 2`) ate
/// twice the discount the real Age A copy (`resourcesForMilitaryUnits: 1`)
/// printed, silently crediting the player 1 extra resource that then
/// compounds turn over turn into an `IllegalMove: Build`/`Upgrade`/
/// `WonderStep` many rounds later. Unlike [`trailing_gets_science`] this
/// cannot use `rfind(" gets ")`: Patriotism's OWN line has a SECOND, later
/// `"gets 1 military action"` clause that would win a last-match search --
/// so this anchors on the `" military resource"` suffix first and reads the
/// number immediately before it instead.
fn trailing_gets_military_resource(text: &str) -> Option<i32> {
    let suffix_pos = text.find(" military resource")?;
    let before = &text[..suffix_pos];
    let gets_pos = before.rfind(" gets ")?;
    before[gets_pos + " gets ".len()..].parse().ok()
}

/// The `"<Color> gets N civil action"` clause anywhere in `text` -- used by
/// the civil-action-TOTAL undercount check (docs/REPLAY.md "civil action
/// total" handoff) to net out two IN-TURN refunds that top up the remaining
/// civil-action POOL without changing the standing TOTAL `costs::ca_total`
/// computes, so a naive running sum of `TakeCard` costs over-counts by
/// exactly this much whenever either fires mid-turn:
/// 1. §3 item 7's leader-replacement refund (`"<Color> elects <New> <Old>
///    dies; <Color> gets 1 civil action"`, `apply.rs`'s own "Replacing a
///    leader refunds one civil action" comment).
/// 2. A client-side `PutBack` undo (`"<Color> puts <Card> back in the row
///    <Color> gets N civil action"`), refunding exactly what the matching
///    `Take` charged.
///    Confirmed against 4 real corpus games (`7522895`, `7522128`, `7523414`,
///    `7522543`, all leader-replacement; `7522905`, a `PutBack`) that were
///    false-positive "undercounts" before this netting was added -- every one
///    dissolved to a zero margin once the refund was subtracted, zero
///    remaining discrepancies across the full 1,009-game corpus.
fn trailing_gets_civil_action(text: &str) -> Option<i32> {
    let suffix_pos = text.rfind(" civil action")?;
    let before = &text[..suffix_pos];
    let gets_pos = before.rfind(" gets ")?;
    before[gets_pos + " gets ".len()..].parse().ok()
}

/// The `"up to N resource and/or food"` clause anywhere in `text` --
/// Plunder's own printed cap (`"<Color> plays Plunder against <Color> Your
/// rival loses a total of up to N resource and/or food (your choice). ...`),
/// which pins down EXACTLY which of Plunder's three age-siblings (I/II/III,
/// `TakeFromOpponentBlock.food_and_or_resources` 3/5/7 respectively,
/// `card_table.rs`) was actually played -- confirmed corpus-wide: every
/// `"plays Plunder"` line prints exactly one of `3`/`5`/`7` here, with no
/// other occurrence of the phrase on the same line to collide with. Unlike
/// [`best_age_sibling`]'s "highest age not exceeding the current one" bound,
/// this is the exact printed number, not a guess bounded by `state.
/// age_military` -- which matters because `age_military` can be AHEAD of
/// the copy actually in a player's hand (a card drawn at an earlier age and
/// never played survives an age transition unplayed), so the bound can
/// overshoot as easily as `corpus::build_card_index`'s arbitrary pick can
/// undershoot (`analysis/worker_notes_2026-08-14/discardsolve__
/// DISCARDSOLVE.txt`, game `7521906`: the bound picked Age II when the
/// journal's own "up to 3" proves Age I).
fn trailing_up_to_resource_and_or_food(text: &str) -> Option<i32> {
    let suffix = " resource and/or food";
    let suffix_pos = text.find(suffix)?;
    let before = &text[..suffix_pos];
    let marker = "up to ";
    let marker_pos = before.rfind(marker)?;
    before[marker_pos + marker.len()..].parse().ok()
}

/// Plunder's own printed cap for `id`, or `None` for every other card --
/// the field [`trailing_up_to_resource_and_or_food`]'s parsed number is
/// matched against to pick the correct age-sibling.
fn plunder_food_and_or_resources(id: CardId) -> Option<i16> {
    // `if let`, not a `match` with a wildcard arm over `Special` -- this
    // project's lint gate denies `_ =>` over an enum (`cargo clippy
    // --all-targets -- -D warnings`), and `Special` has ~80 variants.
    id.get().special.iter().find_map(|s| {
        if let crate::cards::Special::TakeFromOpponent(block) = s {
            Some(block.food_and_or_resources)
        } else {
            None
        }
    })
}

/// Resolves `card` (a "plays Plunder"/"plays Aggression: Plunder" line's
/// `corpus::build_card_index`-arbitrary same-name pick) to the age-sibling
/// the journal's OWN "up to N resource and/or food" text proves was
/// actually played. A no-op for every other Aggression card (`card`'s
/// family is not Plunder) and a safe no-op (returns `card` unchanged) if
/// `raw_text` carries no such clause or no sibling's printed number matches
/// -- never worse than the pre-correction guess.
///
/// Deliberately NOT `best_age_sibling(card, state.age_military)`: that
/// bound is "highest age not exceeding the current one", which is wrong
/// whenever a card drawn at an earlier age survives an age transition
/// unplayed -- `state.age_military` can already be ahead of the copy
/// actually still sitting in the hand. Confirmed on game `7521906` (line
/// 135, journal's own "up to 3" proving Age I): the bound picked the
/// wrong-NEWER Age II sibling instead. The journal's printed cap pins the
/// age down exactly, with no guessing in either direction, so read that
/// instead of bounding a guess.
fn resolve_plunder_age(card: CardId, raw_text: &str) -> CardId {
    if card.get().base_name != "Aggression: Plunder" {
        return card;
    }
    trailing_up_to_resource_and_or_food(raw_text)
        .and_then(|n| family_siblings(card).into_iter().find(|&id| plunder_food_and_or_resources(id) == Some(n as i16)))
        .unwrap_or(card)
}

/// Resolves `card` (a "plays <Aggression>" line's `corpus::build_card_index`
/// arbitrary same-name pick -- `best_age_sibling`'s own doc comment) to the
/// age-sibling whose OWN printed `military_action_cost` matches the
/// journal's own trailing `"uses N military action"` clause
/// ([`civil_and_military_uses`]) -- the same "read the exact printed number
/// off THIS line instead of guessing" approach [`resolve_plunder_age`] just
/// above already uses for Plunder's own "up to N" cap, generalized to every
/// OTHER Aggression card whose age-siblings differ in cost (confirmed:
/// Raid 1/2/3 MA, `card_table.rs`).
///
/// A no-op (returns `card` unchanged) when:
/// - `card`'s family is Plunder -- [`resolve_plunder_age`]'s own job; all
///   three of Plunder's age-siblings tie at 1 MA (`analysis/worker_notes_
///   2026-08-14/decider__DECIDER.txt`), so cost alone cannot disambiguate
///   them and this function must not fight that one's own correction;
/// - the line carries no `"uses N military action"` clause, or the number
///   does not parse;
/// - zero or more than one sibling shares that exact cost (a genuine tie,
///   or evidence this card's family does not vary by MA cost at all) --
///   never worse than the pre-correction guess in either case.
///
/// Root-caused on game `7523354` line 233: BGO's own printed text is Raid's
/// Age II wording ("...2 urban buildings: one of Age II or older and one of
/// Age I or older...", 2 MA) but the bare-name lookup resolved to
/// `"Aggression: Raid (I)"` (1 MA) -- under-deducting `military_actions` by
/// 1 for the rest of the turn, which cascades into a wrong end-of-turn draw
/// count and, several rounds later, masks a real discard requirement the
/// journal states and this binary's own reconstruction does not (`analysis/
/// worker_notes_2026-08-14/decider__DECIDER.txt`'s STEP 1). Measured
/// corpus-wide: this shape (`SimulatorBug`-tagged discard-oracle divergence)
/// covers roughly 75% of the `StuckPending: decider != expected actor,
/// phase Actions, no pending` bucket.
///
/// Correctly resolving Raid II/III (2/3 MA, 2 age brackets each instead of
/// the old code's wrong 1 MA/1 bracket) used to newly expose a SEPARATE,
/// pre-existing bug: `combat::finish_aggression` enqueues one
/// `QueueItem::Raid` per element of `Special::DestroyUrbanBuildings`'s age
/// list, and each bracket's "destroy UP TO N" (RULES_SPEC.md line 82/98) is
/// genuinely declinable -- but `interact.rs`'s `QueueItem::Raid` handling
/// used to hand that choice to `push_choice` with no way to decline, so its
/// own `auto`+`len()==1` path force-destroyed a lone leftover candidate the
/// instant the FIRST bracket left exactly one standing. Confirmed on human
/// game `7522647` (Orange's own starting `Philosophy`, force-destroyed by
/// Raid II's second bracket with no journal record of it) and the
/// structurally identical Raid-II lines in `7522290` and `7522053`. Fixed at
/// the source, not here: `interact.rs`'s `QueueItem::Raid` now appends a
/// `Keyword::Stop` option to a real Raid card's bracket (never to
/// Terrorism's mandatory, no-loot destroy, which is not "up to" anything),
/// and this file's own `ChoiceKind::Raid` handler in `resolve_intervening`
/// picks `Stop` whenever the journal's `raid_destroys` queue has nothing for
/// THIS bracket -- see that handler's own doc for the full mechanism.
fn resolve_aggression_age(card: CardId, raw_text: &str) -> CardId {
    if card.get().base_name == "Aggression: Plunder" {
        return card;
    }
    let Some(wanted_ma) = civil_and_military_uses(raw_text).1 else {
        return card;
    };
    let matches: Vec<CardId> = family_siblings(card).into_iter().filter(|&id| id.get().military_action_cost as i32 == wanted_ma).collect();
    match matches.as_slice() {
        [only] => *only,
        [] | [_, ..] => card,
    }
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
        Move::Take { .. } | Move::WonderStep { .. } | Move::Pop | Move::PopFree | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => None,
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

/// The exact split BGO logs for an Aggression: Plunder attacker's own
/// `ChoiceKind::PlunderSplit` decision -- e.g. `"Grey produces 4 food; Grey
/// produces 1 resource; Purple spends 4 food; Purple spends 1 resource"`, or
/// (a zero-valued clause is omitted entirely, never printed as "0 food")
/// `"Purple produces 3 resources; Green spends 3 resources"`. Unlike
/// [`parse_standalone_produces`] (a `GainBlock`'s own single-clause,
/// nothing-else-on-the-line shape, used for a DIFFERENT and deterministic
/// kind of "produces" line -- e.g. Foray/Refugees' "and/or" grant,
/// `events::food_or_resources`, which never moves anything FROM another
/// player and so never has a trailing "spends" clause) this always has the
/// attacker's own clause(s) immediately followed by the VICTIM's mirrored
/// "spends" clause(s) -- that trailing "; <OtherColor> spends " is the
/// signature checked for below, and is what tells the two same-shaped lines
/// apart (confirmed against the corpus: every multi-clause "produces" line
/// with a following, differently-coloured "spends" clause is a Plunder
/// resolution; the ones with no such clause are Foray/Refugees grants, see
/// this function's own test for a real counter-example that must NOT
/// match).
fn parse_plunder_split_line(text: &str) -> Option<(Color, i16, i16)> {
    let (attacker, rest) = actor_and_rest(text)?;
    let mut food: i16 = 0;
    let mut resources: i16 = 0;
    let mut cursor = rest.strip_prefix("produces ")?;
    loop {
        let digits_end = cursor.find(|c: char| !c.is_ascii_digit())?;
        if digits_end == 0 {
            return None;
        }
        let n: i16 = cursor[..digits_end].parse().ok()?;
        let tail = &cursor[digits_end..];
        // Plural checked before singular -- "resources" starts with
        // "resource", so the singular check alone would leave a stray "s"
        // glued onto `cursor` and break every subsequent match.
        if let Some(t) = tail.strip_prefix(" resources").or_else(|| tail.strip_prefix(" resource")) {
            resources = n;
            cursor = t;
        } else {
            let t = tail.strip_prefix(" food")?;
            food = n;
            cursor = t;
        }
        let continuation = format!("; {} produces ", attacker.as_str());
        match cursor.strip_prefix(continuation.as_str()) {
            Some(t2) => cursor = t2,
            None => break,
        }
    }
    // `cursor` now sits right after the attacker's own clause(s) -- require
    // the victim's mirrored "spends" clause, the signature that separates a
    // real Plunder resolution from the same-shaped deterministic grant.
    let after_semi = cursor.strip_prefix("; ")?;
    let (victim, victim_rest) = actor_and_rest(after_semi)?;
    if victim == attacker || !victim_rest.starts_with("spends ") {
        return None;
    }
    Some((attacker, food, resources))
}

/// Pre-scans every [`parse_plunder_split_line`] match into a per-attacker
/// FIFO, mirroring [`prescan_gain_produces`]. Unlike that FIFO, a `Plunder`
/// resolution with only ONE feasible split (`interact::offer_plunder_split`'s
/// `plunder_split_options`, `auto: true`) never opens a `Pending::Choice` at
/// all -- so this FIFO can carry entries `resolve_intervening` will never be
/// asked to consume, and popping strictly in order would then hand a LATER
/// genuine choice the wrong split. `resolve_intervening`'s own consumer
/// validates each popped entry against the live choice's options and skips
/// (rather than trusting position) past any that don't match, exactly for
/// this reason.
fn prescan_plunder_splits(lines: &[Line]) -> HashMap<u8, VecDeque<(i16, i16)>> {
    let mut out: HashMap<u8, VecDeque<(i16, i16)>> = HashMap::new();
    for line in lines {
        if let Some((attacker, food, resources)) = parse_plunder_split_line(line.text) {
            out.entry(attacker.seat()).or_default().push_back((food, resources));
        }
    }
    out
}

// ---------------------------------------------------------------------
// Discard-phase hand-size oracle
// ---------------------------------------------------------------------
//
// `corpus::classify` matches BGO's own `"Discard Phase N military card(s)
// must be discarded"` / `"No Discard Phase"` announcement (§6.6 step 1's
// modal, opened for EVERY player EVERY turn, not just when a real discard is
// needed) and classifies it as bare `LineOutcome::Bookkeeping` -- the count
// `N` was never parsed anywhere in this file. This is PUBLIC, legal-to-use
// information (an on-screen phase announcement, not a rival's hand or deck
// order) that states, in the journal's own words, exactly how many cards
// this binary's reconstruction OUGHT to need to discard at that turn,
// independent of whatever `hand_military.len()` happens to compute --
// `docs/REPLAY.md`'s "concrete, unused lead" section this implements.
//
// Validated before use, per this task's own "measure first" instruction
// (a sibling investigation on this project found a DIFFERENT journal field,
// a food total, was descriptive RENDERING text that could be off by one
// against the engine's own correct value -- not every printed number is a
// recorded fact). Two independent pieces of evidence rule that out here:
//
// 1. BGO logs this fact TWICE per turn: the announcement itself, and a
//    separate, unambiguously-a-real-action `"<Color> discards N cards"`
//    resolution line the modal resolves into (`corpus::classify` already
//    recognises this second shape as `ActionClass::Discard`, but -- same
//    bug, independently -- throws ITS OWN count away too, `card: None`,
//    since resolving `Pending::Choice(DiscardMilitary)` only needs to know
//    THAT a discard happened, not how many). Corpus-wide, these two
//    independent renderings of the same fact agree on 45,590 of 46,009
//    (99.1%) individual `(actor, round)` entries; nearly all of the
//    remainder is concentrated in a handful of games rather than spread
//    evenly, consistent with the ALREADY-documented "BGO logs the true
//    final turn's End-of-turn lines twice" artifact (`replay_game`'s own
//    `EndTurn` handling, above) rather than a third, unrelated flaw in one
//    of the two fields. [`prescan_discard_phase_oracle`] uses this
//    agreement as a validity GATE, not just a one-off check: only an entry
//    where both renderings agree is trusted.
// 2. On real game `7522614` (one of the 29 games that currently replay
//    clean through to `state.game_over`), EVERY one of its 30
//    announcement/resolution pairs agree with each other, AND the
//    resolution line's own count (a real, unambiguous action, not
//    descriptive text) disagrees with this binary's own reconstruction at
//    round 4 -- Orange's real hand needed exactly 1 discard there (both
//    BGO renderings say so); this binary's own `hand_military_len` computes
//    2 short of the true limit, entering `discard_excess_military`'s loop
//    TWICE and evicting a card the real game never touched. This is
//    decisive: the journal field is corroborated by an independent,
//    unarguably-real action on a game this binary otherwise gets right end
//    to end, and it disagrees with THIS BINARY's own state, not with itself.
fn parse_discard_phase_announcement(text: &str) -> Option<u32> {
    if text == "No Discard Phase" {
        return Some(0);
    }
    let rest = text.strip_prefix("Discard Phase ")?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: u32 = rest[..digits_end].parse().ok()?;
    let tail = &rest[digits_end..];
    (tail == " military card must be discarded" || tail == " military cards must be discarded").then_some(n)
}

/// BGO's own `"<Color> discards N card(s)"` resolution line -- the real,
/// state-changing confirmation [`parse_discard_phase_announcement`]'s modal
/// resolves into. `corpus::classify` already recognises this shape
/// (`ActionClass::Discard`) but discards its own count (`card: None`, and no
/// numeric field on `Classified` to carry it even if it wanted to) since
/// resolving `Pending::Choice(DiscardMilitary)` never needed the number --
/// re-parsed here, independently, purely as this oracle's own cross-check.
fn parse_discard_count_line(text: &str) -> Option<(Color, u32)> {
    let (actor, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("discards ")?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: u32 = rest[..digits_end].parse().ok()?;
    let tail = &rest[digits_end..];
    (tail == " card" || tail == " cards").then_some((actor, n))
}

/// Builds [`Replayer::discard_phase_oracle`] -- see this section's own
/// module doc above for why an entry is only trusted when
/// [`parse_discard_phase_announcement`] and [`parse_discard_count_line`]
/// independently agree (both zero counts as "No Discard Phase" with no
/// matching resolution line at all). Keyed by the journal's own `round`
/// column text verbatim (not re-parsed to a number) -- the same string
/// [`Replayer::check_discard_phase_oracle`] reads off the `EndTurn` line's
/// own `Line::round` to look an entry up, so the two never need to agree on
/// a numeric parse, only on being the same slice of text.
fn prescan_discard_phase_oracle(lines: &[Line]) -> HashMap<(u8, String), u32> {
    let mut announced: HashMap<(u8, String), u32> = HashMap::new();
    let mut actual: HashMap<(u8, String), u32> = HashMap::new();
    for line in lines {
        if let Some(n) = parse_discard_phase_announcement(line.text) {
            if let Some(actor) = Color::parse(line.color) {
                announced.insert((actor.seat(), line.round.to_string()), n);
            }
        }
        if let Some((actor, n)) = parse_discard_count_line(line.text) {
            *actual.entry((actor.seat(), line.round.to_string())).or_insert(0) += n;
        }
    }
    let mut out = HashMap::with_capacity(announced.len());
    for (key, n) in announced {
        if actual.get(&key).copied().unwrap_or(0) == n {
            out.insert(key, n);
        }
    }
    out
}

/// One journal-cross-validated CULTURE running-total fact this binary's own
/// reconstruction disagreed with -- see [`GameResult::culture_oracle_
/// divergence`]. Mirrors [`DiscardOracleDivergence`]'s shape exactly: BGO's
/// own "End turn ... N culture (now M) ..." line states `M`, the
/// authoritative post-turn running total, independent of this binary's own
/// `state.players[_].culture` bookkeeping -- a perfect oracle, not a derived
/// one, unlike the discard-phase excess above. `last_action_class` is the
/// classification deliverable: the last classified action line's own
/// `ActionClass`, of any actor, strictly before this checkpoint's "End turn"
/// line -- the SYMPTOM location (this checkpoint) is usually several rounds
/// AFTER the true cause; `last_action_class` only narrows down where to
/// start tracing backward from, it does not claim to BE the cause.
pub struct CultureOracleDivergence {
    pub lineno: usize,
    pub actor: &'static str,
    /// BGO's own "(now M)" running total -- ground truth.
    pub journal_now: i32,
    /// This binary's own reconstructed total at the same checkpoint --
    /// `state.last_end_of_turn_culture[actor]`'s snapshot (taken by
    /// `game::resume_end_turn` the instant production finished for `actor`,
    /// see that field's own doc), NOT necessarily the live `state.
    /// players[actor].culture`, which a same-call `advance_turn` cascade
    /// (e.g. a war resolving at the start of the NEXT player's turn) can
    /// already have moved further by the time anything reads it.
    pub reconstructed: i32,
    pub last_action_class: Option<ActionClass>,
}

/// [`CultureOracleDivergence`]'s science twin -- see [`GameResult::
/// science_oracle_divergence`]. `SCIENCE_ORACLE`-gated: BGO's own "End turn
/// ... N science (now M) ..." clause sits right next to the culture clause
/// on the exact same line, same running-total grammar, so this is a literal
/// mirror, not a derived check. `round` (absent from `CultureOracleDivergence`,
/// added here per this pass's own corpus-report deliverable -- "at what round
/// do games first diverge") comes straight off the same `Line::round` column
/// the checkpoint's own "End turn" line carries.
pub struct ScienceOracleDivergence {
    pub lineno: usize,
    pub round: String,
    pub actor: &'static str,
    /// BGO's own "(now M)" running total -- ground truth.
    pub journal_now: i32,
    /// This binary's own reconstructed total at the same checkpoint --
    /// `state.last_end_of_turn_science[actor]`'s snapshot, same
    /// snapshot-before-`advance_turn` timing as `CultureOracleDivergence::
    /// reconstructed`.
    pub reconstructed: i32,
    pub last_action_class: Option<ActionClass>,
}

/// [`PendingCultureCheck`]'s science twin -- same self-deferring shape for a
/// checkpoint captured on an `EndTurn` line still blocked on an open
/// `DiscardMilitary` decision.
struct PendingScienceCheck {
    lineno: usize,
    round: String,
    actor_seat: u8,
    journal_now: i32,
    last_action_class: Option<ActionClass>,
}

/// [`ScienceOracleDivergence`]'s resources twin -- same BGO "(now M)"
/// running-total grammar, `"N resources (now M)"` in place of `"N science
/// (now M)"`, `state.last_end_of_turn_resources` in place of `_science`.
pub struct ResourceOracleDivergence {
    pub lineno: usize,
    pub round: String,
    pub actor: &'static str,
    pub journal_now: i32,
    pub reconstructed: i32,
    pub last_action_class: Option<ActionClass>,
}

/// [`PendingScienceCheck`]'s resources twin.
struct PendingResourceCheck {
    lineno: usize,
    round: String,
    actor_seat: u8,
    journal_now: i32,
    last_action_class: Option<ActionClass>,
}

/// A culture-oracle comparison captured on an `EndTurn` line whose own
/// `economy::end_of_turn` stopped early at `discard_excess_military`
/// (leaving a `Pending::Choice(DiscardMilitary)` open) -- BGO's own
/// `"(now M)"` on that SAME "End turn" line already reflects the
/// POST-production total (BGO prints a turn's final numbers before its own
/// follow-up `"<Color> discards N card(s)"` line, even though the discard
/// must be resolved first per RULES_SPEC's own end-of-turn sequence), but
/// this binary's own `state.players[actor].culture` does NOT yet, because
/// `economy::end_of_turn`'s production steps (2-5) do not run until the
/// discard choice is actually answered -- confirmed by trace on real game
/// `7523350` round 5 (`docs/REPLAY.md`): comparing immediately here read a
/// -1 "divergence" that resolved itself, unrecorded, the very next line,
/// making this the SINGLE LARGEST bucket in the culture-oracle histogram's
/// FIRST run and a pure REPLAYER-INSTRUMENT false positive, not a real
/// culture bug. Deferred instead to [`Replayer::flush_pending_culture_
/// check`], called at the top of every subsequent line's own dispatch, by
/// which point this file's own discard-draining machinery (`resolve_
/// intervening`'s generic pending drain, or `apply_one`'s `Discard` arm via
/// `resolve_discard`) has already run production for real.
struct PendingCultureCheck {
    lineno: usize,
    actor_seat: u8,
    journal_now: i32,
    last_action_class: Option<ActionClass>,
}

/// One journal-cross-validated hand-size fact this binary's own
/// reconstruction disagreed with -- see [`GameResult::discard_oracle_
/// divergence`] and this file's "Discard-phase hand-size oracle" module doc.
pub struct DiscardOracleDivergence {
    pub lineno: usize,
    pub round: String,
    pub age: String,
    pub actor: &'static str,
    /// BGO's own cross-validated truth (see [`prescan_discard_phase_oracle`]).
    pub journal_excess: u32,
    /// This binary's own reconstruction at the same checkpoint:
    /// `max(0, hand_military.len() as i32 - (military_actions + military_hand_limit))`.
    pub reconstructed_excess: u32,
    pub hand_len: usize,
    pub limit: i32,
    /// The SAME excess computed a THIRD, independent way: purely from
    /// journal TEXT, via [`prescan_military_hand_ledger`], against the exact
    /// same `limit` above (never re-derived -- the limit formula was already
    /// cross-validated against `RULES_SPEC.md` and is not itself in
    /// question, see this file's "military hand" sections). Lets a reader
    /// tell apart the two ways `reconstructed_excess` can be wrong: this
    /// value agreeing with `journal_excess` implicates the forward
    /// simulator's OWN `hand_military` bookkeeping at a locatable event
    /// (`ledger_last_event` names it); this value ALSO disagreeing means an
    /// event class isn't modelled even by a pure journal reading, which
    /// `hand_ledger_verdict`/[`GameResult::hand_ledger_verdict`] classify.
    pub ledger_excess: u32,
    /// The most recent [`LedgerEventKind`] [`prescan_military_hand_ledger`]
    /// recorded for this actor strictly before this checkpoint's own "End
    /// turn" line, and the 1-based line number it happened at -- `None` only
    /// when this actor has no ledger-tracked event at all yet this game
    /// (rare: the ledger sees every draw, so this is empty only in the
    /// literal opening rounds before any draw has happened).
    pub ledger_last_event: Option<(LedgerEventKind, usize)>,
}

/// One class of journal-observable event [`prescan_military_hand_ledger`]
/// tracks as changing a player's `hand_military` SIZE (never identity -- see
/// that function's own module doc for why the journal proves counts, not
/// which card). Kept as a closed enum, not a string, so
/// [`GameResult::hand_ledger_verdict`]'s corpus-wide histogram
/// (`bin/replaystats.rs`) buckets by a real Rust `match`, not text
/// normalisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerEventKind {
    /// `"<Color> draws N military card(s)"`, wherever it appears: an
    /// ordinary end-of-turn draw (glued onto that round's own "End turn"
    /// line), an event's immediate effect (Development of Politics' "each
    /// player draws 3", Politics of Strength's "strongest draws 5"), or a
    /// colonization territory reward (`apply_immediate_effects`'
    /// `imm.draw_military_cards`, e.g. Strategic Territory's 5) -- all three
    /// render with the exact same clause shape.
    Draw,
    /// `"<Color> discards N card(s)"` -- the discard-phase resolution
    /// [`prescan_discard_phase_oracle`] already cross-validates the COUNT
    /// of, reused here as a ledger event so the running total reflects it
    /// too.
    Discard,
    /// A named play that consumes a card from the player's OWN hand by
    /// identity (`DeclareWar`/`PlayAggression`/`ProposePact`/`PlayTactic`
    /// excluding `CopyTactic`) -- the same predicate
    /// [`prescan_future_military_needs`] uses for [`DiscardSolver`].
    ConsumingPlay,
    /// `"<Color> plays event"` -- §5.2's `PrepareEvent`: moves a card OUT of
    /// hand into `future_events`, banking culture. See `docs/REPLAY.md`'s
    /// "`PrepareEvent`'s net-zero push" section for why this is a real -1,
    /// not a wash, and why `resolve_political_decision`'s own push-then-pop
    /// sequence used to net to zero instead.
    PrepareEvent,
    /// A committed card on a `"<Color> defends ..."` / `"<Color> tries to
    /// defend ..."` line (§5.4.4: a defender may spend a Bonus card or
    /// discard a flat military card to raise their strength) -- EITHER
    /// clause shape (`"N Defense card +B played"` or `"N military card
    /// played"`) counts, one consumed card per unit of `N`. Deliberately
    /// counted on BOTH BGO phrasings, unlike `resolve_aggression_defense`'s
    /// own `parse_defense_clauses`, which only recognises the `"defends "`
    /// prefix and silently reads `"tries to defend"` as zero committed --
    /// see [`defense_consumed_count`]'s own doc for why this ledger does not
    /// share that function (to avoid touching validated, in-use replay
    /// logic) and does not paper over the gap either.
    DefenseConsume,
    /// A `Bonus`/`CookDiscard` clause on a `"<Color> colonizes ..."` line
    /// (§11.3: a colonization sacrifice may include a colonization-bonus
    /// card or a James Cook flat military-card discard, alongside units,
    /// which are NOT hand cards and do not count here) -- reuses
    /// [`parse_sacrifice_clauses`] directly rather than re-parsing.
    ColonizeConsume,
    /// `"Christopher Columbus discovers <Age> / <Territory>"` -- Columbus's
    /// leader ability, a political action that removes Columbus from play to
    /// colonize a territory already sitting in the actor's OWN military hand
    /// "without sacrificing any units" (`corpus::ActionClass::
    /// ColumbusColonize`'s own doc, quoting `bga_throughtheages_material.inc.
    /// php`). "Without sacrificing units" is not "without leaving the hand":
    /// the territory card itself still moves out of `hand_military` into the
    /// player's colonies (`apply.rs::h_columbus_colonize`'s own
    /// `hand_military.remove_first(card)`, proven by this file's
    /// `every_card_consuming_action_class_nets_hand_military_down_by_exactly_
    /// one` test) -- a real -1, same shape as [`LedgerEventKind::
    /// PrepareEvent`], just never routed through this generic dispatch
    /// because its own journal line has NO leading actor colour at all (the
    /// ONLY other line shape sharing that property is `"End turn"`, which
    /// [`prescan_military_hand_ledger`] already special-cases the same way).
    /// Found chasing the `UnmodelledEvent`/`PrepareEvent` ledger bucket
    /// (`docs/REPLAY.md`): a missed Columbus consumption left the ledger's
    /// own running count permanently one card too high from that point
    /// onward, surfacing as a divergence at whatever the NEXT checkpoint
    /// happened to be -- routinely a later `PrepareEvent`, purely because
    /// preparations are frequent, not because `PrepareEvent` itself was ever
    /// the actual cause.
    ColumbusConsume,
}

/// The always-on classification of this game's FIRST discard-phase-oracle
/// divergence (mirrors [`GameResult::civil_deck_premature_advance`]'s own
/// "structural field, set at most once, from the first occurrence" shape) --
/// this is the task's own deliverable: turning "the reconstruction drifted"
/// into "drifted for THIS reason". `None` when this game never diverged, or
/// stopped (a `Mismatch`) before any checkpoint could.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandLedgerVerdict {
    /// The journal-only ledger reproduces the journal's own cross-validated
    /// truth at this checkpoint exactly, while the forward SIMULATOR's own
    /// `hand_military.len()` does not -- this implicates the simulator's own
    /// state bookkeeping (a specific event mishandled, not a missing event
    /// class), e.g. a `ground_*` call that GROWS the hand instead of
    /// consuming a real card (the already-diagnosed `PrepareEvent`
    /// net-zero-push shape, `docs/REPLAY.md`, recurring at a DIFFERENT call
    /// site than the one already fixed).
    SimulatorBug,
    /// The journal-only ledger ALSO disagrees with the journal's own
    /// cross-validated truth -- an event class this project does not model
    /// AT ALL, even reading the journal directly (as opposed to a specific
    /// simulator bug). Carries the most recent [`LedgerEventKind`] this
    /// actor had before the checkpoint (`None` only if there was none yet),
    /// the strongest available clue to WHICH class is missing: if the
    /// ledger's own ADD/SUBTRACT bookkeeping around that event were
    /// complete, it would have matched the journal.
    UnmodelledEvent(Option<LedgerEventKind>),
    /// The ledger had no entry at all for this `(actor, round)` -- should be
    /// rare (the ledger records every `"End turn"` line unconditionally) and
    /// is kept as its own variant, not folded into a guess, so a corpus-wide
    /// nonzero count here is itself a finding about ledger coverage, not
    /// silently mixed into `UnmodelledEvent`.
    NoLedgerEntry,
}

/// BGO's own `"<Color> draws N military card(s)"` clause -- the literal
/// journal fact backing [`LedgerEventKind::Draw`]. Same shape and parsing
/// style as [`parse_discard_count_line`] (this file's own established
/// pattern for a "<Color> <verb> N <noun>(s)" clause), deliberately applied
/// per CLAUSE (the caller splits a line's full text on `"; "` first, not
/// just once against the whole line) because a single line can carry
/// several of these for DIFFERENT colours at once -- an event's "each
/// player draws N military cards" immediate effect glues one
/// `"<Color> draws N military cards"` clause per surviving player onto the
/// SAME `"<Color> plays event ..."` line that also, separately, removes a
/// card from the PREPARING player's own hand ([`LedgerEventKind::
/// PrepareEvent`]).
fn parse_military_draw_clause(text: &str) -> Option<(Color, u32)> {
    let (actor, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("draws ")?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let n: u32 = rest[..digits_end].parse().ok()?;
    let tail = &rest[digits_end..];
    (tail == " military card" || tail == " military cards").then_some((actor, n))
}

/// How many hand-military cards a `"<Color> defends ..."` / `"<Color> tries
/// to defend ..."` line committed, and who committed them -- the ledger-only
/// count backing [`LedgerEventKind::DefenseConsume`]. Deliberately a FRESH
/// parser, not a call to [`parse_defense_clauses`] (which `resolve_
/// aggression_defense` already uses, live, to resolve `Pending::Defense`):
/// that function requires a literal `"defends "` prefix and returns `None`
/// -- read by its own caller as "zero committed" -- for BGO's OTHER
/// phrasing, `"tries to defend"` (used, empirically, when the defender's own
/// committed force still loses the fight). This ledger counts BOTH
/// phrasings, because the rule-mandated card loss happens either way; this
/// is a deliberate, DOCUMENTED difference from production replay behaviour,
/// not a shared bug -- changing `parse_defense_clauses` itself would alter
/// `resolve_aggression_defense`'s own resolution of a REAL `Pending::
/// Defense`, which is out of this instrument's scope (it only measures, see
/// this file's own module doc on the discard-phase oracle for the same
/// discipline). Same two clause shapes as `DefenseClause`, but only the
/// COUNT is kept -- identity plays no part in a hand-SIZE ledger.
fn defense_consumed_count(text: &str) -> Option<(Color, u32)> {
    let (actor, rest) = actor_and_rest(text)?;
    let clauses = match rest.strip_prefix("defends ") {
        Some(c) => c,
        None => rest.strip_prefix("tries to defend")?.strip_prefix("; ").unwrap_or(""),
    };
    let mut n = 0u32;
    for clause in clauses.split("; ") {
        let mut words = clause.split_whitespace();
        let Some(count) = words.next().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let rest_words: Vec<&str> = words.collect();
        let consumed = match rest_words.as_slice() {
            ["Defense", "card", bonus, "played"] => bonus.starts_with('+'),
            ["military", "card", "played"] => true,
            _ => false,
        };
        if consumed {
            n += count;
        }
    }
    Some((actor, n))
}

/// This actor's running `hand_military` SIZE, per
/// [`prescan_military_hand_ledger`], as of some point in the journal --
/// `raw` is intentionally signed and NOT clamped at 0 (see that function's
/// own doc: a negative running total is itself a finding, not noise to hide)
/// -- and the most recent event that changed it, for attributing a
/// divergence to a specific mechanism rather than just a bare number.
#[derive(Debug, Clone, Copy)]
struct LedgerCheckpoint {
    raw: i32,
    last_event: Option<(LedgerEventKind, usize)>,
}

/// A text-only, per-(actor, round) `hand_military` SIZE ledger -- the whole
/// journal solved as a constraint system UP FRONT, rather than inferred by
/// forward-simulating and comparing counts after the fact. See this file's
/// "Discard-phase hand-size oracle" section for why a hand-size drift is a
/// bookkeeping question with a perfect oracle, not a card-identity search:
/// every event that changes `hand_military`'s SIZE (as opposed to its
/// contents) is directly observable in the journal's own words --
/// [`LedgerEventKind`] enumerates the five classes this function tracks,
/// each backed by a literal clause shape, never a rules reimplementation of
/// draw/discard FORMULAS (the existing `military_actions_unused`/
/// `military_hand_limit` machinery is not touched or re-derived here, and
/// was already independently cross-validated against `RULES_SPEC.md` -- see
/// `docs/REPLAY.md`'s "military hand" sections).
///
/// Keyed exactly like [`prescan_discard_phase_oracle`] (`actor` seat,
/// `Line::round` verbatim string), recorded at the identical moment
/// [`Replayer::check_discard_phase_oracle`] reads `hand_military.len()`:
/// the running total is snapshotted right BEFORE each `"End turn"` line's
/// own effects are applied, so that round's OWN draw (textually glued onto
/// the SAME "End turn" line -- `"End turn <Color> scores: ...; <Color>
/// draws N military cards"`) is correctly excluded from ITS OWN checkpoint
/// and instead lands in the NEXT round's running total, mirroring
/// `economy::end_of_turn` running discard (step 1) strictly before draw
/// (a later step) within one turn's own resolution.
///
/// A single pass over `lines`, in order: every line's text is split on
/// `"; "` and each clause is tried against [`parse_military_draw_clause`]
/// (add) and [`parse_discard_count_line`] (subtract) -- this uniformly
/// covers an ordinary end-of-turn draw, an event's "each player
/// draws"/"weakest discards" immediate effect, and a colonization's
/// territory-reward draw with the SAME two clause parsers, because BGO
/// renders all of them with the same clause shape regardless of trigger.
/// Line-level (not clause-level) checks separately handle a named
/// hand-consuming play (`ConsumingPlay`), a `"plays event"` preparation
/// (`PrepareEvent`), a defense commitment (`DefenseConsume`), and a
/// colonization sacrifice's Bonus/Cook-discard clauses (`ColonizeConsume`).
///
/// **A discard's own line order is NOT stable** (confirmed against real
/// corpus text, game `7523347` round 4: `"Discard Phase 2..."` ->
/// `"Green discards 2 cards"` -> `"End turn Green scores: ..."`, discard
/// BEFORE End turn there, the REVERSE of the far more common `"Discard
/// Phase..."` -> `"End turn..."` -> `"<Color> discards..."` order this
/// function's own module doc assumes elsewhere in the corpus -- an
/// already-documented BGO UI-submission-timing artifact, see this file's
/// "Discard-phase hand-size oracle" module doc). A discard resolution whose
/// OWN round has not been checkpointed yet is therefore held in
/// `deferred_same_round_discard` rather than applied to `running`
/// immediately, and flushed right after that round's checkpoint is
/// recorded -- so which textual order BGO happened to pick can never change
/// which round's checkpoint a discard counts against.
fn prescan_military_hand_ledger(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> HashMap<(u8, String), LedgerCheckpoint> {
    let mut running: HashMap<u8, i32> = HashMap::new();
    let mut last_event: HashMap<u8, (LedgerEventKind, usize)> = HashMap::new();
    let mut checkpointed: std::collections::HashSet<(u8, String)> = std::collections::HashSet::new();
    let mut deferred_same_round_discard: HashMap<(u8, String), (i32, usize)> = HashMap::new();
    let mut out = HashMap::new();
    for line in lines {
        if line.text.starts_with("End turn") {
            if let Some(actor) = Color::parse(line.color) {
                let seat = actor.seat();
                let key = (seat, line.round.to_string());
                out.insert(
                    key.clone(),
                    LedgerCheckpoint { raw: *running.get(&seat).unwrap_or(&0), last_event: last_event.get(&seat).copied() },
                );
                checkpointed.insert(key.clone());
                if let Some((n, lineno)) = deferred_same_round_discard.remove(&key) {
                    bump_ledger(&mut running, &mut last_event, seat, n, LedgerEventKind::Discard, lineno);
                }
            }
        }
        // Columbus's own discovery line, the other no-leading-actor-colour
        // shape (`corpus::ActionClass::ColumbusColonize`'s own doc) -- must
        // be special-cased exactly like `"End turn"` above, since the
        // generic `actor_and_rest`-driven dispatch below silently never
        // fires for it at all (`actor_and_rest` requires a leading colour
        // and this line has none). See [`LedgerEventKind::ColumbusConsume`]'s
        // own doc for why this is a real -1, found chasing the
        // `UnmodelledEvent`/`PrepareEvent` ledger bucket.
        if line.text.starts_with("Christopher Columbus discovers ") {
            if let Some(actor) = Color::parse(line.color) {
                bump_ledger(&mut running, &mut last_event, actor.seat(), -1, LedgerEventKind::ColumbusConsume, line.lineno);
            }
        }
        for clause in line.text.split("; ") {
            if let Some((color, n)) = parse_military_draw_clause(clause) {
                bump_ledger(&mut running, &mut last_event, color.seat(), n as i32, LedgerEventKind::Draw, line.lineno);
            }
            if let Some((color, n)) = parse_discard_count_line(clause) {
                let key = (color.seat(), line.round.to_string());
                if checkpointed.contains(&key) {
                    bump_ledger(&mut running, &mut last_event, color.seat(), -(n as i32), LedgerEventKind::Discard, line.lineno);
                } else {
                    let entry = deferred_same_round_discard.entry(key).or_insert((0, line.lineno));
                    entry.0 -= n as i32;
                }
            }
        }
        if let Some((color, n)) = defense_consumed_count(line.text) {
            if n > 0 {
                bump_ledger(&mut running, &mut last_event, color.seat(), -(n as i32), LedgerEventKind::DefenseConsume, line.lineno);
            }
        }
        if let Some((_territory, clauses)) = parse_sacrifice_clauses(line.text, card_index) {
            let n = clauses.iter().filter(|c| !matches!(c, SacrificeClause::Unit(_))).count();
            if n > 0 {
                if let Some((actor, _)) = actor_and_rest(line.text) {
                    bump_ledger(&mut running, &mut last_event, actor.seat(), -(n as i32), LedgerEventKind::ColonizeConsume, line.lineno);
                }
            }
        }
        if let LineOutcome::Action(Classified { class, .. }) = classify(card_index, line.text) {
            if let Some((actor, rest)) = actor_and_rest(line.text) {
                if let Some(kind) = ledger_event_kind_for_action_class(class, rest) {
                    bump_ledger(&mut running, &mut last_event, actor.seat(), -1, kind, line.lineno);
                }
            }
        }
    }
    out
}

/// Which [`LedgerEventKind`] (if any) a `classify`d action-phase line
/// represents, for [`prescan_military_hand_ledger`]'s generic
/// `"<Color> <verb>..."` dispatch (the `actor_and_rest`-gated block just
/// above). EXHAUSTIVE over every `ActionClass` variant, no wildcard arm --
/// the same discipline `action_class_grounds_and_consumes_a_card` already
/// established for the SIMULATOR side (this file's "structural follow-up"
/// section) -- so a new `corpus.rs` variant fails to compile here until
/// someone decides whether it moves a real card out of `hand_military`.
///
/// `ActionClass::ColumbusColonize` is deliberately classified `None` HERE
/// even though it genuinely does consume a card
/// (`every_card_consuming_action_class_nets_hand_military_down_by_exactly_
/// one` proves it on the simulator side): its own journal line has NO
/// leading actor colour at all, so `actor_and_rest` above already rejects it
/// before this function is ever called for it -- seeing this function is
/// therefore not what's missing for that class. It is a real
/// [`LedgerEventKind::ColumbusConsume`], counted by
/// [`prescan_military_hand_ledger`]'s own dedicated `"Christopher Columbus
/// discovers "` check instead, mirroring how `"End turn"` gets its own
/// dedicated check for the identical reason.
fn ledger_event_kind_for_action_class(class: ActionClass, rest: &str) -> Option<LedgerEventKind> {
    use ActionClass::*;
    match class {
        DeclareWar | PlayAggression | ProposePact => Some(LedgerEventKind::ConsumingPlay),
        PlayTactic => (!rest.starts_with("adopts existing tactics ")).then_some(LedgerEventKind::ConsumingPlay),
        PlayEvent => Some(LedgerEventKind::PrepareEvent),

        // Every other class either never touches `hand_military` at all, or
        // (`Discard`) is already counted by this file's own dedicated
        // per-CLAUSE `parse_discard_count_line` pass above -- routing it
        // through here too would double-count it. `ColumbusColonize`: see
        // this function's own doc above.
        TakeCard | BuildBuilding | BuildUnit | BuildWonderStage | IncreasePopulation | UpgradeUnit | UpgradeProduction
        | DevelopTechnology | ElectLeader | ChangeGovernment | WinWar | AcceptPact | Colonize | Discard | Bid | WinAuction
        | Destroy | Disband | Pass | PlayActionCard | PutBack | EndTurn | RemoveLeaderYellow | ColumbusColonize | Barbarossa
        | BachTheater => None,
    }
}

/// Applies one [`LedgerEventKind`] delta to [`prescan_military_hand_ledger`]'s
/// two parallel per-actor running maps at once (the raw signed count and the
/// "what happened most recently" attribution) -- pulled out purely so that
/// function's own body reads as five clearly-separated event classes rather
/// than five copies of the same two-line update.
fn bump_ledger(
    running: &mut HashMap<u8, i32>,
    last_event: &mut HashMap<u8, (LedgerEventKind, usize)>,
    actor: u8,
    delta: i32,
    kind: LedgerEventKind,
    lineno: usize,
) {
    *running.entry(actor).or_insert(0) += delta;
    last_event.insert(actor, (kind, lineno));
}

/// Roll a guessed food/resources gain-or-loss back out of `p` (restoring the
/// pre-`apply_move` snapshot `pre_food`/`pre_res`/`pre_food_tokens`/
/// `pre_resource_tokens`) and apply the JOURNAL's own true `(jf, jr)` split
/// instead, through the real [`economy::gain_food`]/[`economy::gain_resources`]/
/// [`economy::pay_food`]/[`economy::pay_resources`] chokepoints -- never a
/// bare `p.food =`/`p.resources =`.
///
/// TOKENCOUNT fix (2026-08-14): the correction this backs (`resolve_political_
/// decision`'s `triggers_food_or_resources` block, a few hundred lines up)
/// used to overwrite `p.food`/`p.resources` directly, leaving `p.food_tokens`/
/// `p.resource_tokens` (the per-denomination placement history --
/// `crate::state::TokenBank`'s own doc) holding whatever `events::
/// food_or_resources`'s own default guessed split had already credited/
/// debited through the real chokepoints. Once the scalar is corrected but the
/// bank is not, `economy::occupied_tokens` either silently drops the excess
/// (corrected total BELOW the guessed-and-tracked value: it only tops up on
/// an INCREASE, see its own doc) or re-packs a phantom top-up on top of an
/// already-wrong bank (corrected total ABOVE it) -- either way the tracked
/// token COUNT permanently diverges from a real single consistent placement
/// history, even though the corrected VALUE is right. Restoring the FULL
/// pre-guess snapshot before applying the journal's own split keeps bank and
/// scalar in lockstep the same way every other gain/loss site already must.
/// `pay_food`/`pay_resources` (not another restore-then-subtract) are not an
/// option to roll the guess back with: those spend LOWEST denomination
/// first, which would strip pre-existing OLDER tokens instead of undoing the
/// exact ones the guess just placed.
#[allow(clippy::too_many_arguments)]
fn apply_journal_food_or_res_correction(
    p: &mut PlayerState,
    pre_food: u16,
    pre_res: u16,
    pre_food_tokens: crate::state::TokenBank,
    pre_resource_tokens: crate::state::TokenBank,
    jf: i16,
    jr: i16,
    lose: bool,
) {
    p.food = pre_food;
    p.resources = pre_res;
    p.food_tokens = pre_food_tokens;
    p.resource_tokens = pre_resource_tokens;
    if lose {
        if jf > 0 {
            economy::pay_food(p, jf as u16);
        }
        if jr > 0 {
            economy::pay_resources(p, jr as u16);
        }
    } else {
        if jf > 0 {
            economy::gain_food(p, jf as u16);
        }
        if jr > 0 {
            economy::gain_resources(p, jr as u16);
        }
    }
}

/// Foray/Raiders' own `Special::StrongestPlayers`/`WeakestPlayers` "gains/
/// loses a total of N resources and/or food (their choice)" resolution line:
/// `"<Color> produces <N> food; <Color> produces <M> resources"` (either
/// clause, either order, a zero-valued clause omitted entirely -- same
/// convention [`parse_plunder_split_line`] documents). Chasing the
/// `IllegalMove: Pop` bucket (`docs/REPLAY.md`) found this event resolution
/// is a genuine per-player CHOICE, not a deterministic rule: BGO's own line
/// for game `7523357` (Foray, round 8) reads `"Grey produces 2 food; Grey
/// produces 1 resource"` while `blue_available` had 13 tokens free, nowhere
/// near a cap that would force ANY of it into food, and the preceding
/// `"Green and Grey each produce 3 resources and/or food; Grey choses
/// first"` clause on the triggering event line says so outright. The engine
/// has since been given that real choice (`ChoiceKind::FoodOrRes`,
/// replacing the deleted `events::food_or_resources` fixed formula), but a
/// choice made by the BOT is still not the choice the HUMAN made -- so this
/// parser lets the REPLAYER read the human's ACTUAL split off this line and
/// overwrite whatever the engine picked, the same "journal is ground truth"
/// pattern as `TradeResourceAsFood`/`ground_auction_winner_hand`.
///
/// Same clause-parsing loop as [`parse_plunder_split_line`], but the
/// opposite gate on what follows: THAT parser requires a trailing victim
/// `"; <OtherColor> spends "` clause (the signature of a real Plunder
/// resolution); this one requires there be NONE -- a Foray/Raiders grant
/// never takes anything from another player, so nothing ever follows the
/// actor's own clause(s). Returns `None` for a Plunder line (a trailing
/// `"; <OtherColor> spends "` is present) or anything else that isn't
/// exactly one-or-two `"<Color> produces ..."` clauses for a single actor.
fn parse_produces_grant_line(text: &str) -> Option<(Color, i16, i16)> {
    let (actor, rest) = actor_and_rest(text)?;
    let mut food: i16 = 0;
    let mut resources: i16 = 0;
    let mut cursor = rest.strip_prefix("produces ")?;
    loop {
        let digits_end = cursor.find(|c: char| !c.is_ascii_digit())?;
        if digits_end == 0 {
            return None;
        }
        let n: i16 = cursor[..digits_end].parse().ok()?;
        let tail = &cursor[digits_end..];
        // Plural checked before singular -- same trap `parse_plunder_split_
        // line`'s own doc comment documents ("resources" starts with
        // "resource").
        if let Some(t) = tail.strip_prefix(" resources").or_else(|| tail.strip_prefix(" resource")) {
            resources = n;
            cursor = t;
        } else {
            let t = tail.strip_prefix(" food")?;
            food = n;
            cursor = t;
        }
        let continuation = format!("; {} produces ", actor.as_str());
        match cursor.strip_prefix(continuation.as_str()) {
            Some(t2) => cursor = t2,
            None => break,
        }
    }
    // A trailing victim clause means this is really a Plunder resolution --
    // `parse_plunder_split_line`'s line to read, not this one.
    if !cursor.is_empty() {
        return None;
    }
    Some((actor, food, resources))
}

/// Pre-scans every [`parse_produces_grant_line`] match into a per-actor
/// FIFO, mirroring [`prescan_plunder_splits`]. Consumed by the `PrepareEvent`
/// handling in [`Replayer::resolve_political_decision`] to correct
/// `food_or_resources`'s deterministic guess -- see that call site and
/// [`parse_produces_grant_line`]'s own doc for why a correction, not a real
/// `Pending::Choice`, is this bucket's fix.
fn prescan_produces_grants(lines: &[Line]) -> HashMap<u8, VecDeque<(i16, i16)>> {
    let mut out: HashMap<u8, VecDeque<(i16, i16)>> = HashMap::new();
    for line in lines {
        if let Some((actor, food, resources)) = parse_produces_grant_line(line.text) {
            out.entry(actor.seat()).or_default().push_back((food, resources));
        }
    }
    out
}

/// [`parse_produces_grant_line`]'s LOSS-side mirror: `Special::WeakestPlayers`
/// events (Raiders, Crime Wave) resolve their own `Special::Lose(food_and_or_
/// resources)` block the identical way -- a real, sequential, per-player
/// choice ("`<Color> choses first`" on the triggering event line), not
/// `events::food_or_resources`'s fixed "resources first" formula.
///
/// REPLAYER BUG (found extending the `IllegalMove: Build` bucket's
/// `resources_short` trace, `docs/REPLAY.md`'s handoff, game `7522886`
/// round 6/7): `resolve_political_decision`'s existing correction loop
/// already GATES on both `Special::StrongestPlayers`/`WeakestPlayers` and
/// both `Special::Gain`/`Lose`, but its own delta check
/// (`delta_food < 0 || delta_res < 0`) unconditionally skips every NEGATIVE
/// delta -- so a loss (`WeakestPlayers`) never actually got corrected, only
/// gains did. Confirmed via full arithmetic reconciliation against a THIRD,
/// independent number (not just the two conflicting clauses on the
/// triggering event line): game `7522886`'s Orange enters round 7 with 3
/// resources (round 7's own "now 3" end-of-turn total minus round 7's own
/// spending, both journal-observed), fully untouched by the preceding
/// Raiders line's "Orange loses 2 resources and/or food" -- Orange's OWN
/// resolution line, `"Orange spends 2 food"`, put the whole loss on food,
/// the exact opposite of `events::food_or_resources`'s "resources first".
///
/// Same clause-parsing loop and same "no trailing victim clause" gate as
/// [`parse_produces_grant_line`] (a bare, standalone loss line never takes
/// anything from another player either, so nothing ever follows the
/// actor's own clause(s)), just matching `"spends "` instead of
/// `"produces "`. `corpus::classify` already resolves a standalone
/// `"<Color> spends N food"` line to `LineOutcome::Bookkeeping` (the same
/// catch-all `"spends "` prefix an ordinary action's OWN embedded `"spends"`
/// clause never reaches, since `actor_and_rest`'s `rest` for a real action
/// line starts with the action's own verb, e.g. `"builds Warrior Orange
/// spends 2 resources"` -- `strip_prefix("spends ")` on THAT rest correctly
/// fails), so this is safe to scan the whole journal for without a second,
/// separate classification pass.
fn parse_spends_grant_line(text: &str) -> Option<(Color, i16, i16)> {
    let (actor, rest) = actor_and_rest(text)?;
    let mut food: i16 = 0;
    let mut resources: i16 = 0;
    let mut cursor = rest.strip_prefix("spends ")?;
    loop {
        let digits_end = cursor.find(|c: char| !c.is_ascii_digit())?;
        if digits_end == 0 {
            return None;
        }
        let n: i16 = cursor[..digits_end].parse().ok()?;
        let tail = &cursor[digits_end..];
        if let Some(t) = tail.strip_prefix(" resources").or_else(|| tail.strip_prefix(" resource")) {
            resources = n;
            cursor = t;
        } else {
            let t = tail.strip_prefix(" food")?;
            food = n;
            cursor = t;
        }
        let continuation = format!("; {} spends ", actor.as_str());
        match cursor.strip_prefix(continuation.as_str()) {
            Some(t2) => cursor = t2,
            None => break,
        }
    }
    if !cursor.is_empty() {
        return None;
    }
    Some((actor, food, resources))
}

/// Pre-scans every [`parse_spends_grant_line`] match into a per-actor FIFO,
/// mirroring [`prescan_produces_grants`]. Consumed the same way, for the
/// negative-delta (loss) half of the same correction loop.
fn prescan_spends_grants(lines: &[Line]) -> HashMap<u8, VecDeque<(i16, i16)>> {
    let mut out: HashMap<u8, VecDeque<(i16, i16)>> = HashMap::new();
    for line in lines {
        if let Some((actor, food, resources)) = parse_spends_grant_line(line.text) {
            out.entry(actor.seat()).or_default().push_back((food, resources));
        }
    }
    out
}

/// The resolving line for an Aggression: Infiltrate attack that actually
/// removed something -- BGO prints this shape under EITHER of two leading
/// phrases (confirmed by pairing every real corpus `"plays Infiltrate
/// against"` line with whichever line downstream actually carries the
/// consequence, `docs/REPLAY.md`'s six-pending-kind pass): the VICTIM's own
/// `"concedes defeat <Card> is killed; <Attacker> scores N culture"` (a
/// leader) or `"...is destroyed; ..."` (a wonder) when the two land on one
/// combined line, OR -- when the victim has genuinely nothing to answer
/// with, mirroring `Pending::Defense`'s own forced 0-defender shape -- a
/// BARE `"concedes defeat"` from the victim (no clause at all, correctly
/// unparsed by this function -- there is nothing here to read) immediately
/// followed by the ATTACKER's own `"Operation successful <Card> is
/// killed/destroyed; <Attacker> scores N culture"` line carrying the SAME
/// information. Since this only ever scans for the "is killed"/"is
/// destroyed" + "scores N culture" shape wherever it lands, both cases are
/// read identically without needing to special-case the split. Confirmed
/// unambiguous: no OTHER line in the sampled corpus contains "is killed" or
/// "is destroyed" at all, and no OTHER card/event ever leads with
/// "Operation successful".
fn parse_infiltrate_line(text: &str) -> Option<(Color, bool)> {
    if !text.starts_with("concedes defeat") && !text.starts_with("Operation successful") {
        return None;
    }
    let (after, is_wonder) = if let Some(after) = find_after(text, " is killed; ") {
        (after, false)
    } else {
        let after = find_after(text, " is destroyed; ")?;
        (after, true)
    };
    let (attacker, rest) = actor_and_rest(after)?;
    if !rest.starts_with("scores ") {
        return None;
    }
    Some((attacker, is_wonder))
}

/// `text[text.find(needle)? + needle.len()..]`, spelled out because `Option`
/// chaining through both a `find` and a slice add doesn't fit one line
/// cleanly.
fn find_after<'a>(text: &'a str, needle: &str) -> Option<&'a str> {
    let pos = text.find(needle)?;
    Some(&text[pos + needle.len()..])
}

/// Parses a `"GAME DATA UPDATED <Color> <stat>: <old> -> <new>[; <Color>
/// <stat>: <old> -> <new> ...]"` line -- BGO's own admin correction, rare (6
/// of 1,011 sampled journals) but real: it rewrites a player's stat mid-game
/// with no player-facing action of its own to replay. `classify` (`corpus.
/// rs`) files the whole line as `LineOutcome::Bookkeeping` (a deliberate,
/// documented no-op for "an admin correction BGO occasionally injects"), so
/// nothing ever applied it -- confirmed by hand-tracing the culture oracle's
/// dominant `TakeCard`-bucket example (`docs/REPLAY.md`, game `7523809` line
/// 353): this reconstruction's own Orange culture reaches 86 right where
/// BGO's `"GAME DATA UPDATED Orange culture: 86 -> 80; Purple culture: 65 ->
/// 71"` line lands, and every later checkpoint is off by exactly the 6 this
/// binary never subtracted. Scoped to `culture` ONLY -- the same line shape
/// also corrects a `science`/`Bronze` clause in other sampled games;
/// `Bronze` (BGO's own internal name for the player's RESOURCE stockpile --
/// confirmed by the shape of every sampled `"<Color> Bronze: <old> -> <new>"`
/// clause, always parallel to the `science`/`culture` clauses on the same
/// line, never near-plausible as a literal card-name reading) now has its
/// own twin parser, [`parse_game_data_updated_resources`], for exactly the
/// same reason this one exists. The former "out of scope" `science` clause
/// now has its own twin too, [`parse_game_data_updated_science`] -- the
/// `IllegalMove: Develop` bucket (`docs/REPLAY.md`, game `7522617`) was
/// traced to exactly that dropped clause.
///
/// Returns one `(Color, old, new)` triple per `"<Color> culture: X -> Y"`
/// clause found, empty when the line is not this shape at all or carries no
/// culture clause. Deliberately returns `old` too, not just the delta: the
/// SAME `7523809` line's Purple clause ("65 -> 71") turned out NOT to be
/// safe to apply blindly -- Purple's own reconstructed culture at that exact
/// journal position is 74 (mid-deferred-production, `flush_pending_culture_
/// check`'s own pending discard not yet resolved), not 65 at all, so BGO's
/// stated `old` and this reconstruction's own live value are answering two
/// different questions at that moment (confirmed by tracing: applying the
/// +6 delta unconditionally there landed on Purple's live culture BEFORE her
/// turn's own deferred production ran, so the later snapshot double-counted
/// it, turning an exact match at line 343 into a new 87-vs-93 divergence).
/// Orange's OWN clause on the very same line ("86 -> 80"), by contrast, DOES
/// match this reconstruction's live value exactly at that position. The
/// caller applies the delta only when `old` matches its own tracked value
/// for that player, skipping (not guessing at) the clauses where it does
/// not -- conservative by construction, and exactly why this returns `old`
/// instead of pre-computing just a delta.
fn parse_game_data_updated_culture(text: &str) -> Vec<(Color, i32, i32)> {
    let Some(after) = text.strip_prefix("GAME DATA UPDATED ") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for clause in after.split("; ") {
        let Some((color, rest)) = actor_and_rest(clause) else { continue };
        let Some(nums) = rest.strip_prefix("culture: ") else { continue };
        let Some((old_str, new_str)) = nums.split_once(" -> ") else { continue };
        let (Ok(old), Ok(new)) = (old_str.trim().parse::<i32>(), new_str.trim().parse::<i32>()) else { continue };
        out.push((color, old, new));
    }
    out
}

/// The resources twin of [`parse_game_data_updated_culture`] -- identical
/// shape and identical conservative "return `old` too" contract, reading
/// `"<Color> Bronze: <old> -> <new>"` clauses instead of `"culture: ..."`
/// ones. `"Bronze"` here is BGO's own internal label for a player's
/// RESOURCE stockpile, not the Bronze tech card: every sampled clause of
/// this shape sits on a `"GAME DATA UPDATED"` line alongside `culture`/
/// `science` clauses (`replay_common`'s own corpus scan, `2026-08-14`), the
/// same "one stat correction per player, semicolon-joined" pattern, and the
/// values move independently of whether the player has ever built/upgraded
/// an actual Bronze mine that game.
fn parse_game_data_updated_resources(text: &str) -> Vec<(Color, i32, i32)> {
    let Some(after) = text.strip_prefix("GAME DATA UPDATED ") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for clause in after.split("; ") {
        let Some((color, rest)) = actor_and_rest(clause) else { continue };
        let Some(nums) = rest.strip_prefix("Bronze: ") else { continue };
        let Some((old_str, new_str)) = nums.split_once(" -> ") else { continue };
        let (Ok(old), Ok(new)) = (old_str.trim().parse::<i32>(), new_str.trim().parse::<i32>()) else { continue };
        out.push((color, old, new));
    }
    out
}

/// The science twin of [`parse_game_data_updated_culture`] -- identical
/// shape and identical conservative "return `old` too" contract, reading
/// `"<Color> science: <old> -> <new>"` clauses instead of `"culture: ..."`
/// ones. This closes the gap the culture twin's own doc named explicitly:
/// the `science` clause of a `"GAME DATA UPDATED"` line was previously
/// dropped as plain `Bookkeeping` because no bucket had been traced to it
/// yet. It now has -- `IllegalMove: Develop` (`docs/REPLAY.md`), game
/// `7522617` line 105 reads `"GAME DATA UPDATED Green science: 7 -> 8;
/// Green Bronze: 4 -> 5"` one line before Green's own `"discovers Monarchy
/// ... loses 8 science"` (line 107), which this binary rejected as illegal
/// with `science=7` -- exactly the un-applied correction's own `old` value,
/// one short of Monarchy's science cost of 8.
fn parse_game_data_updated_science(text: &str) -> Vec<(Color, i32, i32)> {
    let Some(after) = text.strip_prefix("GAME DATA UPDATED ") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for clause in after.split("; ") {
        let Some((color, rest)) = actor_and_rest(clause) else { continue };
        let Some(nums) = rest.strip_prefix("science: ") else { continue };
        let Some((old_str, new_str)) = nums.split_once(" -> ") else { continue };
        let (Ok(old), Ok(new)) = (old_str.trim().parse::<i32>(), new_str.trim().parse::<i32>()) else { continue };
        out.push((color, old, new));
    }
    out
}

/// Pre-scans the whole journal once for every [`parse_infiltrate_line`]
/// match into a per-attacker FIFO, mirroring [`prescan_plunder_splits`] --
/// see `Replayer::infiltrates`'s doc and `resolve_intervening`'s
/// `ChoiceKind::Infiltrate` handling, which drains it.
fn prescan_infiltrates(lines: &[Line]) -> HashMap<u8, VecDeque<bool>> {
    let mut out: HashMap<u8, VecDeque<bool>> = HashMap::new();
    for line in lines {
        if let Some((attacker, is_wonder)) = parse_infiltrate_line(line.text) {
            out.entry(attacker.seat()).or_default().push_back(is_wonder);
        }
    }
    out
}

/// Pre-scans the whole journal once for every `"<Color> destroys <Card>"`
/// line (`ActionClass::Destroy`), per-actor, paired with its own INDEX into
/// `lines` -- see `Replayer::lose_pop_destroys`'s doc and `resolve_
/// intervening`'s `ChoiceKind::LosePop` handling, which drains this FIFO
/// (validated against the live choice's own options, exactly like
/// `prescan_plunder_splits`) only for the out-of-journal-order case. Every
/// entry here is a real, ordinary action line the main replay loop would
/// otherwise translate on its own when reached in order (unlike a
/// `Bookkeeping`-classified line) -- the line index travels with the card so
/// `resolve_intervening` can record it in `claimed_destroy_lines` the
/// instant an entry is actually consumed, and the main loop skips it there
/// rather than double-applying the same destroy.
fn prescan_lose_pop_destroys(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> HashMap<u8, VecDeque<(usize, CardId)>> {
    let mut out: HashMap<u8, VecDeque<(usize, CardId)>> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        // `Disband` alongside `Destroy`: a `LosePop` resolution renders as
        // either verb depending on whether the surrendered worker-holder is
        // a civil card or a military unit (see this FIFO's own consumer,
        // `resolve_intervening`'s `ChoiceKind::LosePop` arm, for the full
        // story) -- both must feed the SAME per-player queue, in journal
        // order, or a `Disband` resolution the fast path missed has nothing
        // here to fall back on either.
        let LineOutcome::Action(Classified { class: ActionClass::Destroy | ActionClass::Disband, card: Some(card) }) =
            classify(card_index, line.text)
        else {
            continue;
        };
        let Some((actor, _)) = actor_and_rest(line.text) else { continue };
        out.entry(actor.seat()).or_default().push_back((i, card));
    }
    out
}

/// The Terrorism event's own destruction line -- `"Terrorists destroy a
/// <Color> <Building>"` -- one per victim, `corpus::classify`'s existing
/// `Bookkeeping` case (grep that string), the destroyed card discarded
/// there today. The victim's colour is read only to skip past it to the
/// building name: which player it belongs to is already pinned by the live
/// `Pending::Choice(Raid)`'s own options, exactly like [`prescan_infiltrates`]
/// doesn't need the victim's identity either.
fn parse_terrorism_destroy_line(index: &HashMap<&'static str, CardId>, text: &str) -> Option<CardId> {
    let after = text.strip_prefix("Terrorists destroy a ")?;
    let (_, after_color) = actor_and_rest(after)?;
    let (id, _) = longest_known_card_prefix(index, after_color)?;
    Some(id)
}

/// Aggression: Raid's own resolution line -- `"Raid casualties 1
/// <Building1>[; 1 <Building2>]; <Attacker> produces <M> resources"`, one
/// clause per printed age tier (1 or 2 `QueueItem::Raid`s per use) --
/// currently `Unclassified` (`corpus::classify` has no case for it at all),
/// also unused. The resource-gain amount needs no separate parsing here --
/// `resolve_choice`'s `ChoiceKind::Raid` arm already computes it
/// deterministically (`printed.div_ceil(2)`) as a side effect of applying
/// the right `Move::Choose` -- only the destroyed buildings' identities feed
/// `resolve_intervening`'s FIFO. [`longest_known_card_prefix`]'s own matched
/// span swallows a glued-on trailing `;` (it is part of the same
/// whitespace-delimited word as the card name, e.g. `"Alchemy;"`) -- so the
/// remainder after each casualty starts with a bare space, not `"; "`; the
/// continuation check below strips that space first and looks for another
/// `"1 "` clause, not a `"; 1 "` one.
fn parse_raid_casualties_line(index: &HashMap<&'static str, CardId>, text: &str) -> Option<Vec<CardId>> {
    let mut cursor = text.strip_prefix("Raid casualties ")?.strip_prefix("1 ")?;
    let mut out = Vec::new();
    loop {
        let (id, rest) = longest_known_card_prefix(index, cursor)?;
        out.push(id);
        cursor = rest.strip_prefix(' ')?;
        match cursor.strip_prefix("1 ") {
            Some(next) => cursor = next,
            None => break,
        }
    }
    // What's left has to be the attacker's own trailing "<Color> produces
    // <M> resources" clause -- the signature that confirms this really was
    // a Raid casualties line and not some other "1 <Card>; 1 <Card>; ..."
    // shape this file has never seen.
    let (_, rest2) = actor_and_rest(cursor)?;
    if !rest2.starts_with("produces ") {
        return None;
    }
    Some(out)
}

/// Pre-scans the whole journal once for every [`parse_terrorism_destroy_line`]
/// / [`parse_raid_casualties_line`] match into ONE GLOBAL FIFO, in journal
/// order -- unlike every other FIFO in this file, NOT split per-player,
/// because Terrorism's own line never names an attacker at all (only a
/// victim, already redundant with the live choice's own `victim` field).
/// See `Replayer::raid_destroys`'s doc and `resolve_intervening`'s
/// `ChoiceKind::Raid` handling, which drains it (validated against the live
/// choice's own options and skipped, not trusted by position, exactly like
/// [`prescan_plunder_splits`] -- a single-candidate Raid also auto-resolves
/// with no `Pending` at all, so this FIFO can carry entries `resolve_
/// intervening` is never asked to consume).
fn prescan_raid_destroys(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> VecDeque<CardId> {
    let mut out = VecDeque::new();
    for line in lines {
        if let Some(id) = parse_terrorism_destroy_line(card_index, line.text) {
            out.push_back(id);
        } else if let Some(ids) = parse_raid_casualties_line(card_index, line.text) {
            out.extend(ids);
        }
    }
    out
}

/// Pre-scans the whole journal once for every `"<Color> loses <Territory>
/// (<Age numeral>)"` line -- the REAL resolution of a multi-colony
/// Independence Declaration `Pending::Choice(LoseColony)`, e.g. `"Purple
/// loses Historic Territory (I)"`. Unlike a bare territory family name (the
/// shape [`ColonizeSacrifice::territory`] reads, ambiguous across the six
/// families' three ages each) this line prints the SAME string
/// `build_card_index` already keys every card's own `name` field by --
/// territory cards are the one family whose `name` bakes the age suffix
/// straight in (`"Historic Territory (I)"`, not a bare family name plus a
/// separately-parsed numeral) -- so a direct index lookup on the line's own
/// trailing text resolves the exact card, no roman-numeral parsing needed.
/// See `Replayer::lose_colonies`'s doc and `resolve_intervening`'s
/// `ChoiceKind::LoseColony` handling, which drains this per-actor.
fn parse_lose_colony_line(index: &HashMap<&'static str, CardId>, text: &str) -> Option<(Color, CardId)> {
    let (actor, rest) = actor_and_rest(text)?;
    let territory_name = rest.strip_prefix("loses ")?;
    let &id = index.get(territory_name)?;
    (id.kind() == CardType::Territory).then_some((actor, id))
}

fn prescan_lose_colonies(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> HashMap<u8, VecDeque<CardId>> {
    let mut out: HashMap<u8, VecDeque<CardId>> = HashMap::new();
    for line in lines {
        if let Some((actor, id)) = parse_lose_colony_line(card_index, line.text) {
            out.entry(actor.seat()).or_default().push_back(id);
        }
    }
    out
}

/// Pre-scans the whole journal once for every `"Ravages of Time <Wonder>
/// crumble(s)"` line -- the REAL resolution of a multi-wonder Ravages of
/// Time `Pending::Choice(FlipWonder)`. This is the one other shape besides
/// `ColumbusColonize` with NO leading colour in the text at all -- `Line::
/// color` (column 2) is the only place the actor is, read via `Color::parse`
/// exactly like that call site. Card names never carry a leading "The " the
/// way the journal's own flavour text does (`"The Pyramids crumble"`,
/// `"The Library of Alexandria crumbles"`), so it is stripped first; without
/// that, [`longest_known_card_prefix`]'s decreasing-word-count search would
/// never find a dictionary hit at all (`"The"` alone is never a card name)
/// and every line would silently fail to parse instead of resolving. See
/// `Replayer::flip_wonders`'s doc and `resolve_intervening`'s
/// `ChoiceKind::FlipWonder` handling, which drains this per-actor.
fn parse_ravages_of_time_line(index: &HashMap<&'static str, CardId>, color: &str, text: &str) -> Option<(Color, CardId)> {
    let actor = Color::parse(color)?;
    let after = text.strip_prefix("Ravages of Time ")?;
    let after = after.strip_prefix("The ").unwrap_or(after);
    let (id, _) = longest_known_card_prefix(index, after)?;
    (id.kind() == CardType::Wonder).then_some((actor, id))
}

fn prescan_flip_wonders(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> HashMap<u8, VecDeque<CardId>> {
    let mut out: HashMap<u8, VecDeque<CardId>> = HashMap::new();
    for line in lines {
        if let Some((actor, id)) = parse_ravages_of_time_line(card_index, line.color, line.text) {
            out.entry(actor.seat()).or_default().push_back(id);
        }
    }
    out
}

/// `"<Color> takes spoils of war <Color> steals <Card> from <Color>"` -- the
/// journal's per-pick resolution of a War over Technology steal (see
/// `Replayer::war_tech_spoils`'s doc for why BGO logs one such line per
/// PICK, not one per war -- `interact::offer_war_tech` re-offers after every
/// steal, and mixing is legal, FAQ p.8). The actor prints TWICE (confirmed
/// across the corpus, e.g. `7522455` line 201: `"Orange takes spoils of war
/// Orange steals Navigation from Purple"`) -- `actor_and_rest` strips the
/// first, the second is checked to actually match rather than assumed to,
/// since a mismatch would mean this shape was misidentified. The victim
/// after `"from "` is not returned: it is already redundant with the live
/// choice's own `victim` field, exactly like Terrorism's destroy line never
/// needing an attacker.
fn parse_war_tech_steal_line(index: &HashMap<&'static str, CardId>, text: &str) -> Option<(Color, CardId)> {
    let (actor, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("takes spoils of war ")?;
    let (actor2, rest) = actor_and_rest(rest)?;
    if actor2 != actor {
        return None;
    }
    let rest = rest.strip_prefix("steals ")?;
    let (id, rest) = longest_known_card_prefix(index, rest)?;
    rest.strip_prefix(" from ")?;
    Some((actor, id))
}

/// `"<Color> takes spoils of war <Color> gets N science; <Color> loses N
/// science"` -- the journal's per-pick resolution of a War over Technology
/// spoils pick landing on science (either the whole advantage, when the
/// loser has nothing stealable, or the remainder after a steal). Only the
/// SHAPE is checked, not the amount `N` -- see [`WarSpoil::Science`]'s own
/// doc for why re-deriving it here would duplicate arithmetic `interact::
/// take_war_science` already does deterministically from the live state.
fn parse_war_tech_science_line(text: &str) -> Option<Color> {
    let (actor, rest) = actor_and_rest(text)?;
    let rest = rest.strip_prefix("takes spoils of war ")?;
    let (actor2, rest) = actor_and_rest(rest)?;
    if actor2 != actor {
        return None;
    }
    let rest = rest.strip_prefix("gets ")?;
    let digits_end = rest.find(|c: char| !c.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    rest[digits_end..].strip_prefix(" science")?;
    Some(actor)
}

/// Pre-scans the whole journal once for every [`parse_war_tech_steal_line`] /
/// [`parse_war_tech_science_line`] match into a per-actor FIFO, in journal
/// order -- the exact sequence `interact::offer_war_tech`'s own re-offer
/// loop produces the picks in, so no validate-and-skip is needed the way
/// `raid_destroys`/`lose_colonies` need it (those FIFOs can also collect
/// entries from an EARLIER, already-auto-resolved single-option choice;
/// `offer_war_tech` never auto-resolves once a real `Pending::Choice` is
/// open -- `push_choice`'s len-1 rule only fires before the choice exists at
/// all, when `war_tech_options` is empty). See `Replayer::war_tech_spoils`'s
/// doc and `resolve_intervening`'s `ChoiceKind::WarTech` handling, which
/// drains this per-actor.
fn prescan_war_tech_spoils(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> HashMap<u8, VecDeque<WarSpoil>> {
    let mut out: HashMap<u8, VecDeque<WarSpoil>> = HashMap::new();
    for line in lines {
        if let Some((actor, id)) = parse_war_tech_steal_line(card_index, line.text) {
            out.entry(actor.seat()).or_default().push_back(WarSpoil::Steal(id));
        } else if let Some(actor) = parse_war_tech_science_line(line.text) {
            out.entry(actor.seat()).or_default().push_back(WarSpoil::Science);
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
            ActionClass::TakeCard | ActionClass::BuildBuilding | ActionClass::BuildUnit | ActionClass::BuildWonderStage | ActionClass::IncreasePopulation | ActionClass::UpgradeUnit | ActionClass::UpgradeProduction | ActionClass::DevelopTechnology | ActionClass::ElectLeader | ActionClass::ChangeGovernment | ActionClass::WinWar | ActionClass::AcceptPact | ActionClass::Colonize | ActionClass::Discard | ActionClass::Bid | ActionClass::WinAuction | ActionClass::Destroy | ActionClass::Disband | ActionClass::Pass | ActionClass::PlayEvent | ActionClass::PlayActionCard | ActionClass::PutBack | ActionClass::EndTurn | ActionClass::RemoveLeaderYellow | ActionClass::ColumbusColonize | ActionClass::Barbarossa | ActionClass::BachTheater => false,
        };
        if consumes_own_hand_card {
            out.entry(actor.seat()).or_default().push(FutureNeed { lineno: line.lineno, card });
        }
    }
    out
}

// ---------------------------------------------------------------------
// Pre-scan: the colonization sacrifice record
// ---------------------------------------------------------------------

/// One thing physically committed to a colonization force, read off its own
/// clause of a `"<Color> colonizes a <Territory> Sacrificed Units:; ..."`
/// line. BGO writes ONE CLAUSE PER COMMITTED PIECE, exactly like the
/// `"<Color> defends ..."` line does (see [`DefenseClause`]) -- the module
/// doc's old claim that this file could not resolve the sacrifice was about
/// effort, not about the journal withholding the facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SacrificeClause {
    /// `"1 Warrior"` / `"1 Knights"` / ... -- an army unit token sacrificed
    /// from the board. The unit type alone is a full identity: each of the
    /// ten unit cards has a distinct name and lives in exactly one age.
    Unit(CardId),
    /// `"1 Colonization card +<n>"` -- `n` is 1, 2 or 3 and is the card's
    /// own printed, unique-per-age identity, the same argument
    /// [`DefenseClause::Bonus`] rests on.
    Bonus(CardId),
    /// `"1 Military card +1"` -- James Cook's discard-for-force. The card
    /// itself is NOT named (any non-bonus hand card qualifies), so only the
    /// COUNT of these is a journal fact.
    CookDiscard,
}

/// The whole force one `"<Color> colonizes ..."` line records, in journal
/// order. `lineno` is only carried for error reporting.
#[derive(Debug, Clone)]
struct ColonizeSacrifice {
    lineno: usize,
    actor: u8,
    /// The territory's name WITHOUT its age -- BGO never prints the age, and
    /// the six territory families each have one card per age I/II/III, so
    /// this is a family name, not a card identity. Used only to confirm that
    /// the queue front really belongs to the auction currently open (see
    /// [`Replayer::ground_auction_winner_hand`]), never to pick a card.
    territory: String,
    clauses: Vec<SacrificeClause>,
}

/// The unit card BGO's sacrifice clause `name` refers to. BGO prints the
/// card's own name verbatim for every unit except `Warriors`, which it
/// writes in the singular; spelled out as a `match` rather than as
/// de-pluralising string surgery so a future unit whose journal spelling
/// differs fails to resolve loudly instead of being mangled into some other
/// card.
fn sacrificed_unit_card(name: &str, card_index: &HashMap<&'static str, CardId>) -> Option<CardId> {
    let card_name = match name {
        "Warrior" => "Warriors",
        other => other,
    };
    let &id = card_index.get(card_name)?;
    id.kind().is_unit().then_some(id)
}

/// The unique card that prints `colonization_bonus == bonus` (1, 2 or 3 --
/// one per age I/II/III). `None` for any other value, which would mean BGO
/// printed a bonus this binary's card table has no card for.
fn colonization_bonus_card(bonus: i16) -> Option<CardId> {
    (0..crate::CARDS.len() as u16)
        .map(CardId)
        .find(|id| id.kind() == CardType::Bonus && id.get().effects.colonization_bonus == bonus)
}

/// Parse every committed-piece clause out of a `"<Color> colonizes a
/// <Territory> Sacrificed Units:; ..."` line. `None` means `text` is not a
/// colonize line at all. Clauses this function does not recognise are
/// SKIPPED, not errors: the same semicolon list also carries the
/// `"Colonization bonus: +N"` state total, the `"Total force: N"`
/// bookkeeping line, the territory's own reward clause, and BGO's
/// deck-reshuffle notice, none of which name a committed card.
fn parse_sacrifice_clauses(
    text: &str,
    card_index: &HashMap<&'static str, CardId>,
) -> Option<(String, Vec<SacrificeClause>)> {
    let (_, rest) = actor_and_rest(text)?;
    let after = rest.strip_prefix("colonizes a ")?;
    let (territory, list) = after.split_once(" Sacrificed Units:; ")?;
    let mut out = Vec::new();
    for clause in list.split("; ") {
        let mut words = clause.split_whitespace();
        let Some(n) = words.next().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let rest_words: Vec<&str> = words.collect();
        let one = match rest_words.as_slice() {
            ["Colonization", "card", bonus] => bonus
                .strip_prefix('+')
                .and_then(|b| b.parse::<i16>().ok())
                .and_then(colonization_bonus_card)
                .map(SacrificeClause::Bonus),
            ["Military", "card", "+1"] => Some(SacrificeClause::CookDiscard),
            // Every OTHER unit card name is one word ("Warrior"/"Cannon"/
            // "Knights"/...), which is why this used to be a `[unit]`
            // single-word pattern -- but "Modern Infantry" (age IV) is two
            // words, and that pattern silently swallowed it into the `_ =>
            // None` catch-all below (this function's own doc: unrecognised
            // clauses are SKIPPED, not errors), which is indistinguishable
            // from "not a unit at all" instead of "a unit whose name has a
            // space in it". Joining unconditionally handles every unit name
            // this binary's own card table has, one word or several,
            // without special-casing "Modern Infantry" by string.
            words => sacrificed_unit_card(&words.join(" "), card_index).map(SacrificeClause::Unit),
        };
        if let Some(one) = one {
            out.extend(std::iter::repeat_n(one, n as usize));
        }
    }
    Some((territory.to_string(), out))
}

/// Drop the first occurrence of `clause` from `owed`, if it is there at
/// all. Used to subtract what the engine has already forced into an open
/// `Pending::Colonize` from what the journal's own clause list says the
/// whole force was -- a multiset difference, so a force with two identical
/// bonus cards in it only cancels one per committed copy.
fn remove_first_clause(owed: &mut Vec<SacrificeClause>, clause: SacrificeClause) {
    if let Some(at) = owed.iter().position(|c| *c == clause) {
        owed.remove(at);
    }
}

/// Every colonization in the journal, in order. One queue for the whole
/// game rather than one per seat: the auctions themselves are strictly
/// sequential (only one `Pending::Auction` can be open at a time), so the
/// front of this queue is always the outcome of the auction currently in
/// progress -- when that auction is won at all.
fn prescan_colonize_sacrifices(
    lines: &[Line],
    card_index: &HashMap<&'static str, CardId>,
) -> VecDeque<ColonizeSacrifice> {
    lines
        .iter()
        .filter_map(|line| {
            let (color, _) = actor_and_rest(line.text)?;
            let (territory, clauses) = parse_sacrifice_clauses(line.text, card_index)?;
            Some(ColonizeSacrifice { lineno: line.lineno, actor: color.seat(), territory, clauses })
        })
        .collect()
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
/// Row/hand cards are USUALLY a unique instance in the table (the same name
/// never denotes two different physical cards within one game -- the same
/// assumption `ground_row_slot`/`card_index` rely on), so a `PutBack` can
/// almost always only refer to a still-open take of that exact card by that
/// exact actor -- matching is done with a per-card stack of not-yet-resolved
/// takes rather than a single "last take" slot: any `TakeCard` pushes onto
/// its card's stack, any `PutBack` pops an entry for the same actor, and
/// (defensively, since it should never happen with unique card instances)
/// any OTHER classified line naming that same card by the same actor -- i.e.
/// the take was committed some other way (built, developed, played, elected,
/// ...) -- removes it from the stack so a later same-named "put back" (which
/// cannot legitimately exist once a card is committed) can never wrongly
/// erase this now-real action.
///
/// The "unique instance" premise breaks for a multi-copy yellow ACTION card
/// (`count` other than `[1,1,1]`, e.g. Frugality (A) at `[2,2,2]` --
/// `can_take_gated`'s own doc: action cards are exempt from the one-per-name
/// rule precisely because several copies can sit in the row at once). Game
/// `7522905` line 17-19: Grey takes Frugality for 2 CA, takes a SECOND
/// physical Frugality (a different row slot) for 1 CA, then puts one back
/// for a refund of 2 CA. Popping the MOST RECENT open take (the old
/// behaviour) skips the 1-CA take instead of the 2-CA one the refund
/// actually matches, so the "kept" take silently becomes the 2-CA card --
/// overcounting this turn's true civil-action spend by 1 and manufacturing a
/// `IllegalMove: Take` `Budget` rejection on this same turn's later, actually
/// affordable, take. The refund clause is direct evidence of which take it
/// undoes: prefer whichever open entry's own recorded cost equals the
/// putback's refund, falling back to "most recent" (the single-copy-safe
/// behaviour) only when no cost match exists.
fn prescan_putback_skips(lines: &[Line], card_index: &HashMap<&'static str, CardId>) -> std::collections::HashSet<usize> {
    let mut skip = std::collections::HashSet::new();
    let mut open: HashMap<CardId, Vec<(usize, Color, i32)>> = HashMap::new();
    for (i, line) in lines.iter().enumerate() {
        let LineOutcome::Action(Classified { class, card }) = classify(card_index, line.text) else {
            continue; // bookkeeping: never references a card, nothing to do
        };
        let Some((actor, _)) = actor_and_rest(line.text) else { continue };
        let Some(c) = card else { continue };
        match class {
            ActionClass::TakeCard => {
                let cost = civil_and_military_uses(line.text).0.unwrap_or(0);
                open.entry(c).or_default().push((i, actor, cost));
            }
            ActionClass::PutBack => {
                if let Some(stack) = open.get_mut(&c) {
                    let refund = trailing_gets_civil_action(line.text);
                    let pos = refund
                        .and_then(|r| stack.iter().rposition(|&(_, a, cost)| a == actor && cost == r))
                        .or_else(|| stack.iter().rposition(|&(_, a, _)| a == actor));
                    if let Some(pos) = pos {
                        let (take_i, _, _) = stack.remove(pos);
                        skip.insert(take_i);
                        skip.insert(i);
                    }
                }
            }
            ActionClass::BuildBuilding | ActionClass::BuildUnit | ActionClass::BuildWonderStage | ActionClass::IncreasePopulation | ActionClass::UpgradeUnit | ActionClass::UpgradeProduction | ActionClass::DevelopTechnology | ActionClass::ElectLeader | ActionClass::ChangeGovernment | ActionClass::PlayTactic | ActionClass::DeclareWar | ActionClass::WinWar | ActionClass::PlayAggression | ActionClass::ProposePact | ActionClass::AcceptPact | ActionClass::Colonize | ActionClass::Discard | ActionClass::Bid | ActionClass::WinAuction | ActionClass::Destroy | ActionClass::Disband | ActionClass::Pass | ActionClass::PlayEvent | ActionClass::PlayActionCard | ActionClass::EndTurn | ActionClass::RemoveLeaderYellow | ActionClass::ColumbusColonize | ActionClass::Barbarossa | ActionClass::BachTheater => {
                // This actor committed ONE physical card by this OTHER
                // route (built/played/developed/...), so at most ONE open
                // take can be the card just committed -- `stack.retain`
                // used to drop EVERY still-open take of this actor's for
                // this card name, which is right for the overwhelmingly
                // common single-copy case (nothing else there to wrongly
                // keep) but wrong the moment a multi-copy card (`count`
                // other than `[1,1,1]`) has TWO takes open for the same
                // actor at once, e.g. one held over from an earlier turn
                // plus one just taken this turn. Game `7522613` line
                // 177/216/217/219: Green takes Reserves (I) for 2 CA
                // (round 6), takes a SECOND Reserves (I) for 1 CA (round
                // 7), then PLAYS Reserves (round 7) and puts one back for
                // a 1 CA refund. `retain` wiped BOTH open takes on the
                // `plays` line, so the later putback found nothing open at
                // all -- `UnrecoverableHiddenInfo: unpaired BGO
                // client-side undo`, not merely a wrong pairing. The
                // putback's own refund (1 CA) is direct evidence only the
                // round-7 take (also 1 CA) survived to be put back, so the
                // round-6 take must be the one this commit consumed --
                // i.e. commits drain the OLDEST open take first (FIFO),
                // mirroring the order the physical cards were actually
                // picked up in, and leaving any newer take(s) open for a
                // later putback to still find and pair against.
                if let Some(stack) = open.get_mut(&c) {
                    if let Some(pos) = stack.iter().position(|&(_, a, _)| a == actor) {
                        stack.remove(pos);
                    }
                }
            }
        }
    }
    skip
}

// ---------------------------------------------------------------------
// Per-game replay
// ---------------------------------------------------------------------

/// One journal "End turn" checkpoint's worth of `cardblame`'s corruption
/// cross-check -- see [`Replayer::record_corruption_check`]'s own doc for
/// how `engine_charges` is computed and why it is safe to read before
/// `economy::end_of_turn` itself has run. `journal_charges` is BGO's own
/// ground truth: does this exact "End turn" line carry a `"CORRUPTION!"`
/// marker. The two agreeing or not is the population `cardblame`'s
/// corruption enrichment cut is built from; `cards_in_play` is the acting
/// player's OWN cards only (their tableau, government, leader, wonder,
/// tactic and won territories), not the whole game's, because a per-player
/// mechanism like blue-token capacity can only plausibly be explained by a
/// card that specific player holds.
pub struct CorruptionCheck {
    pub lineno: usize,
    pub actor: &'static str,
    pub engine_charges: bool,
    pub journal_charges: bool,
    pub cards_in_play: Vec<&'static str>,
}

/// One player's own cards in play, broadly -- tableau technologies,
/// government, leader, the wonder under construction (if any), tactic, and
/// every territory ever won at colonization (append-only, see
/// [`crate::state::PlayerState::colonies`]'s own doc). Used by
/// `cardblame`'s corruption cross-check, where the standing hypothesis under
/// test is a TERRITORY card (a colonization prize, not a tableau tech), so
/// the set must reach beyond `techs` alone. `CardId::NONE` (an empty
/// government/leader/wonder slot) is filtered out rather than named.
fn player_cards_in_play(p: &crate::state::PlayerState) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for (id, _) in p.techs.iter() {
        out.push(id.name());
    }
    for id in [p.government, p.leader, p.wonder, p.tactic] {
        if !id.is_none() {
            out.push(id.name());
        }
    }
    for &id in p.colonies.as_slice() {
        out.push(id.name());
    }
    out
}

/// The narrower set `cardblame`'s MAIN completion/score enrichment cut uses,
/// per the task's own definition: "all players' tableaus, plus leaders and
/// tactics in force" -- deliberately excludes government/wonder/territories
/// that [`player_cards_in_play`] also carries, so a caller wanting the
/// wider set (the corruption cross-check) uses that function directly
/// instead. Deduplicated ACROSS players into one set per game: this answers
/// "was this card in play at all when the game stopped", not "how many
/// copies" -- a card built by both players in a 2p game must not count
/// double in the games-in-play denominator.
fn game_cards_in_play(state: &crate::state::GameState) -> Vec<&'static str> {
    let mut set: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    for p in &state.players[..state.num_players as usize] {
        for (id, _) in p.techs.iter() {
            set.insert(id.name());
        }
        if !p.leader.is_none() {
            set.insert(p.leader.name());
        }
        if !p.tactic.is_none() {
            set.insert(p.tactic.name());
        }
    }
    set.into_iter().collect()
}

/// Every `CardId` argument `mv` names, zero to two of them. Exhaustive over
/// [`Move`] (no wildcard arm, matching this codebase's own lint gate) so a
/// future variant added to `moves.rs` is a compile error here until someone
/// decides what card(s), if any, it names -- the entire point of
/// `game_ever_cards_in_play` being unable to silently miss a new move
/// shape. Response moves that name a card the RESPONDING player played
/// (`Defend`, `SendUnit`, `SendBonus`, `SendDiscard`) are included -- they
/// are real cards leaving a hand, exactly the kind of "played or resolved"
/// exposure task 2026-08-14 part (A) asks for, even though they are not the
/// acting `Move::Aggression`/`Move::Colonize` (there is no such variant --
/// see `Move`'s own module doc) player's own turn.
fn move_card_args(mv: Move) -> [Option<CardId>; 2] {
    use crate::moves::Move::*;
    match mv {
        Take { .. } => [None, None],
        Build { card } => [Some(card), None],
        Develop { card } => [Some(card), None],
        Upgrade { from, to } => [Some(from), Some(to)],
        WonderStep { .. } => [None, None],
        Pop => [None, None],
        PopFree => [None, None],
        Revolution { card } => [Some(card), None],
        PlayLeader { card } => [Some(card), None],
        PlayAction { card } => [Some(card), None],
        Destroy { card } => [Some(card), None],
        PlayTactic { card } => [Some(card), None],
        CopyTactic { card } => [Some(card), None],
        Aggression { card, .. } => [Some(card), None],
        War { card, .. } => [Some(card), None],
        OfferPact { card, .. } => [Some(card), None],
        CancelPact { .. } => [None, None],
        PrepareEvent { card } => [Some(card), None],
        RemoveLeaderYellow => [None, None],
        ColumbusColonize { card } => [Some(card), None],
        Barbarossa { card } => [Some(card), None],
        BachTheater { from, to } => [Some(from), Some(to)],
        TradeFoodAsResource => [None, None],
        TradeResourceAsFood => [None, None],
        Bid { .. } => [None, None],
        BidPass => [None, None],
        Defend { card } => [Some(card), None],
        DefendDone => [None, None],
        SendUnit { card } => [Some(card), None],
        SendBonus { card } => [Some(card), None],
        SendDiscard { card } => [Some(card), None],
        SendDone => [None, None],
        Choose { .. } => [None, None],
        Churchill { .. } => [None, None],
        EndTurn => [None, None],
        PolPass => [None, None],
        Resign => [None, None],
    }
}

/// The WIDENED companion to [`game_cards_in_play`] -- see
/// [`GameResult::ever_cards_in_play`]'s own doc for what this answers and
/// why it is kept alongside, not instead of, the narrow snapshot set.
/// Built from FOUR sources, unioned: (1) [`game_cards_in_play`] itself, so
/// this is always a superset; (2) every `CardId` [`move_card_args`] finds
/// across every `Move` this game ever applied (events, one-shot actions,
/// aggressions/wars/pacts, and superseded leaders/tactics/governments a
/// single snapshot cannot see); (3) every player's own `colonies` list
/// directly (append-only, but colonization assigns the auction winner their
/// territory without any `Move` naming that `CardId` at all, so (2) alone
/// would miss it -- same reason [`player_cards_in_play`] adds it
/// separately for the corruption cross-check); (4) each player's own FINAL
/// `wonder`/`government` slot directly -- neither is EVER named as a
/// `Move` argument: a wonder card enters `p.wonder` as a side effect of
/// `Move::Take` (which only carries a row `slot`, not a `CardId`), and the
/// starting government (`Despotism`) is part of `new_game`'s initial setup,
/// never the result of applying any `Move` at all. Without this source,
/// EVERY wonder and the starting government would read as zero exposure in
/// this "widened" set despite appearing throughout the corpus -- caught by
/// this task's own zero-exposure section (`cardblame`'s coverage summary)
/// showing exactly that gap on the first real run against the corpus, not
/// by inspection beforehand. `government` here is still single-slot (a
/// LATER revolution/redevelopment supersedes it, same artifact as `leader`/
/// `tactic`), so a superseded EARLIER government still needs (2) above
/// (`Move::Revolution`/`Move::Develop`) to be seen; only the STARTING one
/// is invisible to (2), which is exactly what this closes.
fn game_ever_cards_in_play(
    state: &crate::state::GameState,
    moves_applied: &[Move],
    events_resolved: &[CardId],
) -> Vec<&'static str> {
    let mut set: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    for name in game_cards_in_play(state) {
        set.insert(name);
    }
    for &card in events_resolved {
        set.insert(card.name());
    }
    for &mv in moves_applied {
        for card in move_card_args(mv).into_iter().flatten() {
            set.insert(card.name());
        }
    }
    for p in &state.players[..state.num_players as usize] {
        for &id in p.colonies.as_slice() {
            set.insert(id.name());
        }
        for id in [p.wonder, p.government] {
            if !id.is_none() {
                set.insert(id.name());
            }
        }
    }
    set.into_iter().collect()
}

pub struct GameResult {
    pub id: String,
    pub players: u8,
    pub actions_consumed: usize,
    /// Count of the redundant trailing `"discards N card(s)"` lines this
    /// game's own journal logged (BGO's same-second double log of one
    /// end-of-turn discard, skipped as pure confirmations -- see
    /// `Replayer::trailing_discards_lines`).
    pub trailing_discards: usize,
    /// Count of the redundant duplicate `"<Color> builds <Card>"` lines this
    /// game's own journal logged (BGO's same-turn double log of one build,
    /// skipped as pure confirmations -- see `Replayer::duplicate_build_lines`).
    pub duplicate_builds: usize,
    pub completed: bool,
    pub mismatch: Option<Mismatch>,
    pub colonize_approximated: bool,
    /// See [`Replayer::bid_ceilings_grounded`].
    pub bid_ceilings_grounded: u32,
    /// See [`Replayer::hand_full_takes_overridden`].
    pub hand_full_takes_overridden: u32,
    pub engine_scores: Option<Vec<i32>>,
    pub index_scores: Vec<i32>,
    /// The SET of Age III event cards this reconstruction believes were
    /// still pending (`events::pending_final_events`) when
    /// `game::finish_game` ran -- i.e. exactly the cards
    /// `events::evaluate_final_events` scored into `engine_scores`. `None`
    /// under the same condition as `engine_scores` (`completed &&
    /// state.game_over`). Diagnostic-only: lets a caller compare this
    /// reconstruction's own final-event SET against the journal's real
    /// "End of game" announcement without re-deriving engine state.
    pub final_event_cards: Option<Vec<&'static str>>,
    /// §12.5.2 final-scoring award oracle -- see [`FinalEventAwardDivergence`]
    /// and `parse_final_event_journal_amounts`'s own doc. One entry per
    /// (card, seat) where this game's journal states an explicit "Colour
    /// scores/loses N culture" award for a still-pending "Impact of ..."
    /// card AND this reconstruction's own `events::scoring_culture`
    /// disagrees with it. Empty (not `None`) when nothing diverges, INCLUDING
    /// when the game never reached final scoring at all, so a caller can
    /// `.is_empty()` without also checking `completed`.
    pub final_event_award_divergences: Vec<FinalEventAwardDivergence>,
    /// §12.5.2 final-scoring audit aid: the per-card, per-seat award
    /// `final_event_awards_posthoc` computed for this game, in application
    /// order (a ranked card carries two entries per seat -- the
    /// `scoring_culture` award then the `rankingCulture` bonus -- the
    /// journal's line states their COMBINED total). Diagnostic-only, for
    /// the score-divergence queue: it lets a diverging game's residual be
    /// checked against the journal's OWN final lines without re-deriving
    /// the formula by hand. Empty when the game never reached final
    /// scoring.
    pub final_event_awards_computed: Vec<(&'static str, Vec<(u8, i32)>)>,
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
    /// See [`Replayer::discard_oracle_divergence`] and this file's
    /// "Discard-phase hand-size oracle" module doc: the FIRST `(actor,
    /// round)` checkpoint where this game's own reconstructed military-hand
    /// excess disagreed with BGO's own cross-validated `"Discard Phase N
    /// ..."` count.
    pub discard_oracle_divergence: Option<DiscardOracleDivergence>,
    /// See [`Replayer::discard_oracle_checked`]/[`Replayer::
    /// discard_oracle_agreed`] -- how many checkpoints had a trusted journal
    /// entry to compare against, and how many of those this binary's own
    /// reconstruction matched exactly.
    pub discard_oracle_checked: u32,
    pub discard_oracle_agreed: u32,
    /// See [`HandLedgerVerdict`] -- the always-on, structural classification
    /// of `discard_oracle_divergence` (when `Some`): whether the journal-only
    /// military-hand ledger agrees with the journal at that checkpoint
    /// (implicating the forward simulator specifically) or also disagrees
    /// (an unmodelled event class). `None` exactly when `discard_oracle_
    /// divergence` is `None`.
    pub hand_ledger_verdict: Option<HandLedgerVerdict>,
    /// See [`Replayer::culture_oracle_divergence`] and
    /// [`CultureOracleDivergence`]: this game's FIRST "End turn" checkpoint
    /// where this binary's own running `state.players[_].culture` disagreed
    /// with BGO's own cross-validated `"(now M)"` running total, classified
    /// by the `ActionClass` of whatever the last classified action line was,
    /// strictly before the checkpoint.
    pub culture_oracle_divergence: Option<CultureOracleDivergence>,
    /// See [`Replayer::culture_oracle_checked`]/[`Replayer::
    /// culture_oracle_agreed`] -- how many "End turn" lines had a `"(now
    /// M)"` clause to compare against, and how many of those this binary's
    /// own reconstruction matched exactly.
    pub culture_oracle_checked: u32,
    pub culture_oracle_agreed: u32,
    /// [`Replayer::science_oracle_divergence`]'s twin here -- see
    /// [`ScienceOracleDivergence`]. `SCIENCE_ORACLE`-gated: always `None`
    /// unless `debugflags::science_oracle()` was set for this run.
    pub science_oracle_divergence: Option<ScienceOracleDivergence>,
    pub science_oracle_checked: u32,
    pub science_oracle_agreed: u32,
    /// [`Replayer::resource_oracle_divergence`]'s twin here -- see
    /// [`ResourceOracleDivergence`]. `RESOURCE_ORACLE`-gated.
    pub resource_oracle_divergence: Option<ResourceOracleDivergence>,
    pub resource_oracle_checked: u32,
    pub resource_oracle_agreed: u32,
    /// The FIRST line, if any, where this reconstruction's own
    /// `state.age_civil` read strictly ahead of what the journal's `Line::
    /// age` column proves the real game had reached -- see
    /// `PrematureCivilAdvance`'s own doc and `docs/REPLAY.md`'s "civil deck
    /// model" handoff. `None` on every game that never diverges (the
    /// intended state after `top_up_civil_deck` landed; kept as a
    /// structural, always-on instrument rather than removed, so a future
    /// regression shows up in `replaystats`'s own summary instead of
    /// needing this investigation re-run from scratch).
    pub civil_deck_premature_advance: Option<PrematureCivilAdvance>,
    /// A structural "false skip" instrument, the same always-on shape as
    /// [`civil_deck_premature_advance`](Self::civil_deck_premature_advance):
    /// how many times, in this game, `game::auto_skip_politics` closed a
    /// player's Politics phase (leaving `state.phase != Politics`) while the
    /// journal's own solved event-preparation plan (`event_plan::solve`)
    /// says that SAME player had a real preparation waiting whose line had
    /// already been reached. This is the mechanism traced from the
    /// zero-matching final-score cross-check above: a false skip means the
    /// upcoming `"<Color> plays event ... Current event: ..."` line can
    /// never reach [`Replayer::resolve_political_decision`] (the game is no
    /// longer in `Phase::Politics` for them), so it falls through
    /// `apply_one`'s `ActionClass::PlayEvent => Ok(())` arm as a silent
    /// no-op instead -- neither the reveal's own culture bonus nor its
    /// effect ever lands on either player, AND the card is never popped off
    /// `current_events`, so it wrongly fires a SECOND time (with the wrong
    /// amount, computed against end-of-game state instead of the turn it
    /// really resolved on) via `events::evaluate_final_events`. Root cause
    /// is `hand_military` under-tracking (the same class of gap
    /// `d4ad0f5`'s discard-phase oracle instruments, not a NEW one this
    /// counts) leaving zero `CardType::Event`/`Territory` cards in the
    /// player's reconstructed hand at the moment `game::start_turn` calls
    /// `auto_skip_politics`, even though the real hand had one.
    ///
    /// **Read this before "fixing" a nonzero value here (a note for a
    /// reader six months out, this author included):** as of the fix
    /// described in `docs/REPLAY.md`'s "Final scores" section,
    /// `resolve_intervening` RECOVERS every one of these occurrences
    /// immediately, in place -- reopening `Phase::Politics` and calling
    /// `resolve_political_decision` right there, the exact same claim path
    /// an on-time preparation goes through. **A nonzero count here is NOT
    /// damage.** It is a raw occurrence counter for the still-open
    /// `hand_military` under-tracking gap itself (kept, on purpose, as a
    /// regression signal for THAT gap -- redefining it to read zero after
    /// this fix would have thrown that signal away for a tidier-looking
    /// number). If you are hunting for damage, look at
    /// [`politics_false_skips_unrecovered`](Self::politics_false_skips_unrecovered)
    /// instead (the true "recovery actually failed" count, which SHOULD be
    /// investigated if nonzero), or at a new `IllegalMove`/`StuckPending`
    /// bucket / a worse final-score delta -- not at this field moving.
    /// Measured on the full corpus the fix landed against: this field 60
    /// across 57 games, `politics_false_skips_unrecovered` 0, mean
    /// final-score delta -10.54 -> -7.15, exact zeros 8 -> 9 (see
    /// `docs/REPLAY.md`).
    pub politics_false_skips: u32,
    /// The TRUE damage signal, as opposed to `politics_false_skips`'s raw
    /// occurrence count (read that field's own doc first -- it explains why
    /// the two are not the same thing on purpose). Increments only when
    /// [`Replayer::resolve_intervening`]'s own immediate recovery attempt
    /// -- reopening `Phase::Politics` and calling `resolve_political_
    /// decision` on the spot -- itself fails (a genuine `IllegalMove`/
    /// `EventPlanInfeasible`, not the ordinary "auto_skip closed the phase
    /// on schedule" case). That failure also propagates as this game's own
    /// `Mismatch` (this file never silently swallows a real error), so a
    /// nonzero value here always coincides with an early stop for that
    /// game -- this field exists purely so `replaystats`'s own corpus-wide
    /// summary can distinguish "the gap still bites but is recovered" (the
    /// `politics_false_skips` case) from "the recovery itself broke"
    /// without re-deriving which is which from the raw mismatch bucket
    /// table. Zero across the full corpus as of the fix landing.
    pub politics_false_skips_unrecovered: u32,
    /// `cardblame`'s own deliverable: the set of card ids in play (all
    /// players' tableaus, plus leaders and tactics in force -- see
    /// [`game_cards_in_play`]'s own doc for exactly what that does and does
    /// not include) at the point this game stopped or ended. Snapshotted
    /// from `r.state` UNCONDITIONALLY, whether or not the game completed --
    /// a stopped game's cards-in-play at the stop point is exactly what the
    /// enrichment cut needs (see `bin/cardblame.rs`'s own module doc).
    pub cards_in_play: Vec<&'static str>,
    /// `cardblame`'s corruption cross-check population for this game -- see
    /// [`CorruptionCheck`]'s own doc. Empty for a game with no "End turn"
    /// lines at all (shouldn't happen in practice), never `None`, so a
    /// caller can iterate unconditionally.
    pub corruption_checks: Vec<CorruptionCheck>,
    /// Task 2026-08-14 ("fix it to be everything"): the WIDENED companion to
    /// [`cards_in_play`](Self::cards_in_play) -- every card this
    /// reconstruction ever saw PLAYED or RESOLVED at any point in the game,
    /// not merely present at the stop/end snapshot. See
    /// [`game_ever_cards_in_play`]'s own doc for exactly what goes in: every
    /// card named as a `Move` argument (covers one-shot events/actions/
    /// aggressions/wars/pacts that resolve and leave play, and superseded
    /// leaders/tactics/governments a single-slot snapshot cannot see), plus
    /// every territory ever colonized (`PlayerState::colonies` has no `Move`
    /// of its own), UNIONED with [`cards_in_play`](Self::cards_in_play)
    /// itself so this is always a superset, never a disjoint or narrower
    /// view. Kept SEPARATE from `cards_in_play`, not a replacement for it --
    /// they answer different questions (a "still present" card vs a "was
    /// exercised" card) and `cardblame` prints both, clearly labelled.
    ///
    /// **The confound this set has that `cards_in_play` does not**
    /// (`cardblame`'s own module doc has the parallel note for the narrow
    /// set's single-slot artifact): a card played in round 3 stays in this
    /// set for the rest of the game, so a LONGER game accumulates strictly
    /// more of these by construction -- unlike `cards_in_play`, where a
    /// long completed game's leader/tactic slot has usually been
    /// SUPERSEDED, biasing it the other way. `cardblame` prints the mean
    /// round reached for games with/without each card here as it does for
    /// the narrow set, so this confound is visible the same way.
    pub ever_cards_in_play: Vec<&'static str>,
    /// Every `Move` this reconstruction actually applied, in order -- see
    /// [`Replayer::moves_applied`]'s own doc. `cardblame`'s Move-keyed
    /// happenings cut (task 2026-08-14 part 2) matches over this
    /// exhaustively, one counter per `Move` variant, so a future variant
    /// added to `moves.rs` fails that match at compile time until someone
    /// classifies it, rather than silently going uncounted.
    pub moves_applied: Vec<Move>,
    /// Every journal line's own [`ActionClass`], in order -- see
    /// [`Replayer::line_classes`]'s own doc. `cardblame`'s second,
    /// independent happenings cut: several `ActionClass` variants
    /// (`WinAuction`, `WinWar`, `PlayEvent`, `Discard`, ...) are pure
    /// confirmation lines with no `Move` of their own, so this is the only
    /// place those happenings are visible at all.
    pub line_classes: Vec<ActionClass>,
}

/// Ground-truth evidence (never a rules reimplementation) that the PRIMARY,
/// deck-empty-triggered `game::advance_age` call inside `game::deal` fired
/// before the journal itself says the real game reached that age --
/// distinct from (and unreachable-by-construction after) the CORRECTIVE
/// `catch_up_civil_age`/`game::force_civil_age_at_least` path, which only
/// ever moves `state.age_civil` UP TO a line's own stated age, never past
/// it. Built from `Line::age`, a column BGO stamps on every single row, not
/// from any derived/approximated fact.
#[derive(Debug, Clone, Copy)]
pub struct PrematureCivilAdvance {
    /// 1-based journal line number at which the divergence was first
    /// observed (matches `Decision::lineno`/`Mismatch::lineno`'s
    /// numbering).
    pub lineno: usize,
    /// What the journal's own age column said, at that line.
    pub journal_age: crate::cards::Age,
    /// What this reconstruction's `state.age_civil` already was.
    pub reconstructed_age: crate::cards::Age,
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
/// - `WinWar`: `game::start_turn`'s own doc is explicit that war RESOLUTION
///   (`combat::resolve_war_outcome`/`apply_war_spoils`) fires at the START
///   OF THE ATTACKER'S NEXT TURN, not when `DeclareWar` was applied --
///   `apply_one`'s `WinWar` arm is already a bare `Ok(())` "validation
///   checkpoint only" precisely because the real state mutation already
///   happened, synchronously, inside whatever earlier `advance_turn` made
///   the attacker current. BGO's `"<Color> wins War over ..."` line names
///   the WINNER (attacker or defender, whichever the strength comparison
///   favoured), not necessarily the player whose turn is starting, and its
///   timestamp routinely collides (same second) with an unrelated OTHER
///   player's own trailing `"End turn"` line -- confirmed on real game
///   `7523809` line 342 (`"Orange wins War over Culture"`, timestamp
///   `13:03:27`) printed one line BEFORE Purple's own `"End turn"` line at
///   the IDENTICAL timestamp, with no `EndTurn` in between -- the same
///   "not stably ordered within a second" artifact already documented for
///   `WinAuction`/Taj Mahal. Calling `resolve_intervening` for this line
///   sent `expected_actor` to the named winner while `decider` was still
///   whoever's turn was genuinely in progress, with no pending open to
///   explain the gap -- the single largest identified cause of the
///   `StuckPending: decider != expected actor ... no pending` bucket (59 of
///   216 games, `docs/REPLAY.md`).
///
/// See the four call sites' own doc comments (above the single call to
/// this function in `replay_game`'s main loop) for the specific real games
/// each was found on.
fn is_pure_confirmation_line(class: ActionClass) -> bool {
    matches!(class, ActionClass::PlayEvent | ActionClass::WinAuction | ActionClass::Colonize | ActionClass::WinWar)
}

/// Catch `state.age_civil` up to the journal's own age column
/// (`game::force_civil_age_at_least`'s own doc explains why this
/// reconstruction's `civil_deck` can lag the true deck's depletion, and why
/// reading BGO's authoritative age column is correct rather than
/// approximating it). A no-op on every line where this reconstruction's own
/// age already matches or leads (the overwhelmingly common case).
///
/// REPLAYER BUG (found chasing `IllegalMove: Pop`, game `7522648` round 7,
/// `docs/REPLAY.md`'s handoff): BGO logs the age column per LINE, not per
/// turn-in-progress -- a turn whose own `End turn` line is still tagged the
/// OLD age (e.g. "I") can leave a `DiscardMilitary` choice open
/// (`interact::discard_excess_military` returned early, `economy::
/// end_of_turn` not yet run its own production/consumption steps) that only
/// gets drained by `resolve_intervening` while processing the FOLLOWING
/// line, which is already tagged the NEW age (e.g. "II"). Forcing the age
/// (and with it `advance_age`'s §12.2.4 "-2 yellow_bank" deduction, once per
/// surviving player) unconditionally at the top of that following line's own
/// iteration ran it BEFORE the stalled player's own `end_of_turn` had
/// actually completed -- so their OWN round's food consumption was computed
/// off an already-decremented `yellow_bank`, one whole age transition early.
/// Confirmed against game `7522648`: the journal's own "End turn ... 1 food
/// - consumption: 1" for Orange's round-7 turn requires the PRE-deduction
///   `yellow_bank` (13, consumption 1); this binary was computing consumption
///   off the POST-deduction value (11, consumption 2) because the age-II line
///   ("Purple passes Political Phase", the very next line) forced the
///   deduction before `resolve_intervening` had drained Orange's own
///   still-open `DiscardMilitary` choice and let `end_of_turn` finish.
///
/// Deferring the force-forward while ANY decision or deferred effect from an
/// earlier line is still outstanding (`pending`/`queue` nonempty) lets that
/// earlier work complete under the age it actually happened in; the very
/// next line where both are empty again calls this same function (still
/// unconditional up to that point) and brings `state.age_civil` up to date
/// before anything that truly belongs to the new age is applied.
/// `pending`/`queue` are empty at the top of the loop on ordinary lines
/// (each branch's own `resolve_intervening`+`try_apply` pair drains both
/// before the loop moves on), so this is a no-op there, same as the
/// journal-age-already-caught-up no-op above.
///
/// That `pending`/`queue` guard is necessary but not sufficient: it only
/// catches a turn caught MID-interruption. Game `7522064` line 328 is the
/// gap it misses -- BGO's own un-actored "Last turn" bookkeeping line can sit
/// ahead of a DIFFERENT player's still-fully-synchronous (`pending`/`queue`
/// both empty) `End turn`/`discards` trailer, two lines later in the SAME
/// old age, that this function hasn't even been asked to run yet. The
/// caller in `replay_game`'s main loop closes that gap by only calling this
/// function at all on an [`is_trustworthy_age_line`] line -- see that
/// predicate's own doc for why "Last turn" and the `EndTurn`/`Discard`
/// classes are excluded there rather than here: `catch_up_civil_age` itself
/// has no access to a line's classification, only its age string, so the
/// gate belongs at the call site, not inside this function.
fn catch_up_civil_age(state: &mut GameState, journal_age: &str) {
    if state.pending.is_empty() && state.queue.is_empty() {
        if let Some(age) = parse_age(journal_age) {
            game::force_civil_age_at_least(state, age);
        }
    }
}

/// Whether a journal line's OWN age column is trustworthy ground truth for
/// bounding `state.age_civil` -- shared by `last_real_decision_line_for_age`
/// (backward check, below: "is there still more of the OLD age's real
/// business to come") and `catch_up_civil_age`'s call site in
/// `replay_game`'s main loop (forward check: "is it safe to force the age up
/// to what THIS line claims"). Both need the IDENTICAL answer, or one
/// becomes a stealth duplicate of the other's rule with its own, silently-
/// diverging exception list -- the "hidden twin" shape this module's history
/// keeps rediscovering (this predicate itself used to be exactly that: two
/// copies, one only wired into the checker below).
///
/// Untrustworthy: `LineOutcome::Bookkeeping` (BGO's un-actored trailer
/// lines -- "Last turn", "Discard Phase", "No Discard Phase", ... -- can be
/// exported ahead of the still-old-age turn they trail: game `7522064` line
/// 328's "Last turn", already tagged the new age, precedes Purple's own
/// still-Age-III `End turn`/`discards` two lines later, so forcing off it
/// ran §12.2.4's "-2 yellow_bank" a whole turn early and computed Purple's
/// OWN round's food consumption off the already-decremented value) and the
/// `EndTurn`/`Discard` action classes:
///
/// - `EndTurn` -- false-positived on nearly every game's very first Age A ->
///   I transition (`docs/REPLAY.md`'s "civil deck model" handoff has the
///   full trace, game `7523818` line 8): BGO logs the NEXT player's own
///   "Action Phase begins" marker for their round-2 turn (already tagged the
///   new age) at the SAME timestamp as, and BEFORE, the PREVIOUS player's
///   own trailing "End turn ... scores: ..." line for the round that just
///   ended (still tagged the OLD age/round, correctly describing when it
///   happened).
/// - `Discard` -- the SAME shape, one step removed: `apply_one`'s own
///   `ActionClass::Discard` arm resolves an outstanding `DiscardMilitary`
///   pending, and its own doc comment is explicit that draining the LAST
///   queued discard can itself finish the actor's end of turn and advance
///   `state.current` -- i.e. this line can be the actual TRIGGER for a real
///   age transition, not just adjacent to one. Confirmed on game `7522652`
///   line 430 (`"Green discards 2 cards"`, tagged age `III` round `16`):
///   BGO logs both players' `"Last turn Game ends..."` §12.3 notices
///   (already tagged `IV`) at the SAME timestamp, two lines EARLIER in the
///   file, purely an export-ordering artifact.
///
/// File order is therefore not a reliable total order exactly at a real
/// transition, only within a single, real, non-wrap-up decision. Every
/// buggy divergence `last_real_decision_line_for_age`'s own instrument
/// exists to catch (`7523449`) persists across many such real decisions, not
/// just one trailing line, so this restriction loses no real positive
/// (`last_real_decision_line_for_age_ignores_an_end_turn_trailer_still_
/// tagged_the_old_age`, `last_real_decision_line_for_age_ignores_a_discard_
/// resolution_trailer_still_tagged_the_old_age`,
/// `last_real_decision_line_for_age_still_sees_a_real_decision_tagged_the_
/// old_age`, and `catch_up_civil_age_is_deferred_by_a_bookkeeping_last_turn_
/// line_that_precedes_the_old_ages_own_trailing_end_turn`, below, pin all
/// four).
///
/// ALSO untrustworthy, found chasing the culture oracle's `WinWar` bucket on
/// real game `7523079` (Purple's round-12 `End turn`, line 211, journal `73`
/// vs this binary's `61`, delta -12): every [`is_pure_confirmation_line`]
/// class, not just `EndTurn`/`Discard`. That predicate's OWN doc already
/// establishes `WinWar`'s timestamp "routinely collides (same second) with
/// an unrelated OTHER player's own trailing `End turn` line" and is "not
/// stably ordered within a second" -- the exact same export-ordering
/// artifact this function exists to guard against, just not previously
/// wired in here. Confirmed on `7523079`: Orange's `"wins War over
/// Territory"` (line 210) is tagged age `III` round `13` -- Orange's NEXT
/// turn, where `game::start_turn` actually resolves the war -- but it is
/// printed (same timestamp, `13:53:28`) one line BEFORE Purple's own still-
/// Age-`II` round-12 `End turn` (line 211). Before this fix, that untrusted
/// `III` tag forced `state.age_civil` up a whole turn early, running
/// `advance_age`'s §12.2.4 "-2 yellow_bank" deduction for BOTH players
/// before Purple's own end-of-turn ran -- `happy_required`'s band jumped
/// from 3 to 4 off the prematurely-decremented bank, `discontent` (4 - 3)
/// exceeded Purple's 0 free workers, `economy::end_of_turn` treated it as a
/// genuine uprising (§6.3) and skipped step 3 (culture, science, corruption,
/// food, resource production) entirely for a turn BGO's own journal shows
/// producing normally -- the exact -12 culture / -2 science shortfall,
/// reusing the one predicate rather than growing a second, silently-
/// diverging exclusion list (this function's own history: see the "hidden
/// twin" note on `is_trustworthy_age_line`'s sibling above).
fn is_trustworthy_age_line(outcome: LineOutcome) -> bool {
    matches!(
        outcome,
        LineOutcome::Action(Classified { class, .. })
            if !matches!(class, ActionClass::EndTurn | ActionClass::Discard) && !is_pure_confirmation_line(class)
    )
}

/// Whether `outcome` is a line this file's own turn-boundary machinery can
/// print between the DEFENDER's last old-age line and a `WinWar`
/// confirmation with no real business of its own -- BGO's "Action Phase
/// begins"/"No Discard Phase"/"Discard Phase N cards must be discarded" +
/// the resolving discard, or the `EndTurn` trailer itself. Every one of
/// these is already excluded from [`is_trustworthy_age_line`]; named
/// separately here because [`upcoming_confirmed_winwar_age`] needs to SKIP
/// over them (not just refuse to trust them), to find the `WinWar`
/// confirmation they can sit in front of.
fn is_end_of_turn_bridge_line(outcome: LineOutcome) -> bool {
    matches!(outcome, LineOutcome::Bookkeeping)
        || matches!(outcome, LineOutcome::Action(Classified { class: ActionClass::EndTurn | ActionClass::Discard, .. }))
}

/// The Napoleon Bonaparte / War-over-Culture fix's own discriminator: does
/// journal line `i` sit directly in front of a `WinWar` confirmation this
/// reconstruction has genuinely not caught up to yet, with nothing left of
/// the OLD age still to come?
///
/// `combat::resolve_war_outcome` fires SYNCHRONOUSLY inside
/// `game::start_turn`, itself triggered as a side effect of applying the
/// DEFENDER's own last old-age `EndTurn`/`Discard` line -- there is no
/// journal line boundary between that trailer and the attacker's own
/// synchronous start-of-turn cascade for this file to hook into, so by the
/// time the loop reaches the `WinWar` line itself (already excluded from
/// [`is_trustworthy_age_line`] for an unrelated reason -- see its own doc)
/// the war has already resolved off whatever `state.age_civil` this
/// reconstruction happened to hold. Confirmed directly on real game
/// `7522590`: processing its line 317 (Orange's `EndTurn`, tagged Age III)
/// synchronously resolves Purple's War over Culture while `state.age_civil`
/// is still III, even though the very next two lines are `"No Discard
/// Phase"` (still III) and then `"Purple wins War over Culture"` (tagged
/// IV) -- BGA's own age genuinely turned over between the two.
///
/// This function scans forward from `i`, skipping [`is_end_of_turn_bridge_line`]
/// lines only, and returns the first `WinWar` line's own age if one is found
/// before anything else -- the age this reconstruction has not caught up to
/// yet that the SAME synchronous cascade this bridge belongs to is about to
/// use. Anything else (a real decision line reached before any `WinWar`, or
/// the journal ending first) means there is nothing to trust here, so
/// `None`.
///
/// Deliberately does NOT also require the line AFTER the `WinWar` to avoid
/// an older age (the check `is_trustworthy_age_line`'s own WinWar exclusion
/// needs for its OWN, much wider blast radius -- see that predicate's doc on
/// game `7523079`): real game `7523045` line 335 has EXACTLY that shape --
/// its `WinWar` (Purple, IV) is immediately followed by Orange's own
/// still-Age-III `End turn`/discard trailer (lines 336-337), the identical
/// same-timestamp export artifact `7523079` has -- yet it is a genuine,
/// confirmed (instrumented) case of the age having truly turned over, not a
/// collision to refuse. The two are indistinguishable by "what tag trails
/// the WinWar line" alone; what actually differs is what each caller DOES
/// with the trust. `is_trustworthy_age_line`'s caller forces `state.age_civil`
/// itself plus §12.2.4's cross-player "-2 yellow_bank" deduction plus the
/// deck rebuild off it -- exactly what corrupted `7523079`'s Purple, an
/// unrelated bystander whose OWN not-yet-run production the premature
/// deduction back-dated. This function's caller only ever calls
/// [`game::antiquate_leader_wonder_pacts_up_to`], which touches ONLY the
/// (attacker-or-defender) player who owns the too-old leader/wonder/pact
/// being removed, never `age_civil`/`civil_deck`/`yellow_bank`'s §12.2.4
/// deduction -- there is no shared, cross-player state left for a same-
/// timestamp tie to corrupt.
fn upcoming_confirmed_winwar_age(journal: &[Line], i: usize, card_index: &HashMap<&'static str, CardId>) -> Option<crate::cards::Age> {
    let mut j = i;
    loop {
        let line = journal.get(j)?;
        let outcome = classify(card_index, line.text);
        if matches!(outcome, LineOutcome::Action(Classified { class: ActionClass::WinWar, .. })) {
            return parse_age(line.age);
        }
        if !is_end_of_turn_bridge_line(outcome) {
            return None;
        }
        j += 1;
    }
}

/// The LAST journal line index still tagged each age -- ground truth for
/// `civil_deck_premature_advance`'s "is there still more of the OLD age to
/// come" check. Indexed by `Age as usize` (five ages, `A` through `IV`)
/// rather than a `HashMap`: a fixed, exhaustive, tiny domain is exactly the
/// case this project's own style guide reserves arrays for over a hash
/// table. Built ONLY from [`is_trustworthy_age_line`] lines -- see its own
/// doc for why the rest cannot be trusted to bound anything.
fn last_real_decision_line_for_age(journal: &[Line], card_index: &HashMap<&'static str, CardId>) -> [Option<usize>; 5] {
    let mut last: [Option<usize>; 5] = [None; 5];
    for (i, line) in journal.iter().enumerate() {
        let Some(age) = parse_age(line.age) else { continue };
        if !is_trustworthy_age_line(classify(card_index, line.text)) {
            continue;
        }
        last[age as usize] = Some(i);
    }
    last
}

/// Comfortably above the largest single-line draw this file's own turn loop
/// can trigger (`ROW_SIZE`, one full `game::replenish`), with headroom for
/// the rare case `resolve_intervening` drains more than one stalled turn
/// while processing a single line. `top_up_civil_deck` maintains this as a
/// floor, never a target -- it only ever tops UP, and only when under it.
const CIVIL_DECK_SAFETY_FLOOR: usize = 2 * crate::state::ROW_SIZE;

/// Keep `state.civil_deck` from ever running out ON ITS OWN during replay --
/// the fix for `docs/REPLAY.md`'s "civil deck model" handoff, which the
/// module doc's own history explains in full. Short version: this
/// reconstruction's civil ROW is forced card-by-card from each observed
/// "takes ... in hand" line (`Replayer::ground_row_slot`), never drawn
/// through `civil_deck` -- so `civil_deck`'s own SIZE was never anything
/// more than an approximation, and that approximation was shown to drift in
/// BOTH directions: it can lag the true deck's depletion (an Age I card
/// still legally in a full hand many rounds after the real BGO client had
/// already antiquated it away -- the original `HandFull` handoff), and it
/// can run dry EARLY (`game::deal`'s own embedded `advance_age` firing a
/// full turn before the journal's own age column agrees -- game `7523449`,
/// the "Second `IllegalMove: Pop` pass" handoff).
///
/// Both directions trace to the SAME irreducible information gap, not two
/// separate bugs: BGO's journal states the exact civil-action COST a Take
/// paid, which narrows the real row slot to a same-cost TIER
/// (`costs::row_cost`'s three price bands) but never to the exact slot
/// within it -- and at every player count the CHEAPEST tier (slots 0-4)
/// straddles `game::replenish`'s own mandatory-sweep boundary (`sweep_n` is
/// 3, 2, or 1 for 2/3/4 players, always inside that band). Which side of
/// that boundary the real card sat on determines whether ITS OWN vacancy
/// gets absorbed for free by the next mandatory sweep or costs an extra
/// draw -- and that is genuinely unrecoverable from a cost-tier observation
/// alone, not a gap this file's own parsing can close (confirmed empirically:
/// `Replayer::ground_row_slot` reports a same-cost TIE, several candidate
/// slots that all reproduce the journal's own stated cost, on very nearly
/// every single Take in the corpus).
///
/// Rather than let an irreducibly approximate SIZE keep the power to fire
/// `game::advance_age` early OR late, this keeps `civil_deck` topped up with
/// extra, never-observed filler drawn from the SAME age's own card pool
/// (`game::build_deck`, reshuffled -- statistically the same shape of
/// filler `game::deal` already uses, just more of it) so `deal`'s embedded
/// trigger becomes structurally UNREACHABLE during replay: the floor is
/// checked every line, before anything on that line can draw, and is always
/// comfortably above what one line's worth of engine activity can consume.
/// `catch_up_civil_age`, reading `Line::age` directly, becomes the ONE
/// mechanism left standing for every civil age transition during replay --
/// not a primary approximation plus a corrective snap-forward that can
/// disagree with each other, which is what let `7523449`'s early advance
/// slip past the original snap-forward-only fix (it only ever moves the age
/// UP TO the journal's column, so it structurally cannot have caused an
/// EARLY advance -- see `docs/REPLAY.md`).
///
/// Self-play is untouched: this function's only call site is this file's own
/// per-line loop, and `game::deal`/`game::advance_age`/`game::replenish`
/// keep exactly their existing behaviour, still correct for a real game
/// whose `civil_deck` empties in real time with no lag possible.
fn top_up_civil_deck(state: &mut GameState) {
    if state.civil_deck.len() >= CIVIL_DECK_SAFETY_FLOOR {
        return;
    }
    // §12.3: Age IV has no civil deck at all (`game::advance_age`'s own
    // `nxt == Age::IV` branch empties it outright and it is never dealt
    // from again) -- nothing to top up, and `game::build_deck(Age::IV,
    // true, _)` would just return an empty list.
    if state.age_civil == crate::cards::Age::IV {
        return;
    }
    let mut rng = game::rng_for(state);
    let mut reserve = game::build_deck(state.age_civil, true, game::live_count(state));
    crate::rng::shuffle_cards(rng.get(), reserve.as_mut_slice());
    while state.civil_deck.len() < CIVIL_DECK_SAFETY_FLOOR {
        // `reserve` is one age's own full card pool -- comfortably larger
        // than the floor at every player count (checked by
        // `top_up_civil_deck_reserve_batch_is_never_smaller_than_the_floor`,
        // below). Stopping rather than looping forever if that ever stopped
        // holding is more useful than an infinite loop: the `debug_assert`
        // at this function's call site is what actually catches the
        // regression.
        let Some(card) = reserve.pop() else { break };
        state.civil_deck.push(card);
    }
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
    crate::tie_context::set_game(&meta.id);
    let lines = parse_lines(journal_text);
    let putback_skips = prescan_putback_skips(&lines, card_index);
    let gain_produces = prescan_gain_produces(&lines);
    let plunder_splits = prescan_plunder_splits(&lines);
    let infiltrates = prescan_infiltrates(&lines);
    let lose_pop_destroys = prescan_lose_pop_destroys(&lines, card_index);
    let raid_destroys = prescan_raid_destroys(&lines, card_index);
    let lose_colonies = prescan_lose_colonies(&lines, card_index);
    let flip_wonders = prescan_flip_wonders(&lines, card_index);
    let future_military_needs = prescan_future_military_needs(&lines, card_index);
    let colonize_sacrifices = prescan_colonize_sacrifices(&lines, card_index);

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
    let mut r = Replayer::new(
        card_index,
        meta.players,
        plan,
        gain_produces,
        plunder_splits,
        infiltrates,
        lose_pop_destroys,
        raid_destroys,
        lose_colonies,
        flip_wonders,
        future_military_needs,
        colonize_sacrifices,
    );
    r.record_decisions = record_decisions;
    r.produces_grants = prescan_produces_grants(&lines);
    r.war_tech_spoils = prescan_war_tech_spoils(&lines, card_index);
    r.discard_phase_oracle = prescan_discard_phase_oracle(&lines);
    r.military_hand_ledger = prescan_military_hand_ledger(&lines, card_index);
    r.spends_grants = prescan_spends_grants(&lines);

    // Civil-action-TOTAL undercount check (docs/REPLAY.md "civil action
    // total" handoff): per-actor running sum of every `TakeCard` line's own
    // `"uses N civil action"` clause since that actor's last `EndTurn` --
    // see the check itself, below, for why this is safe ground truth (Take
    // is never a free action) rather than a rules reimplementation.
    let mut ca_take_spend_this_turn: Vec<i32> = vec![0; meta.players as usize];

    let last_line_index_for_age = last_real_decision_line_for_age(journal, card_index);
    let mut civil_deck_premature_advance: Option<PrematureCivilAdvance> = None;

    'lines: for (i, line) in journal.iter().enumerate() {
        if std::env::var("YBANK_DEBUG").is_ok()
            && (line.text.contains("End turn")
                || line.text.starts_with("End of turn")
                || line.text.contains("End of game"))
        {
            for pi in 0..r.state.num_players {
                let p = &r.state.players[pi as usize];
                eprintln!(
                    "YBANK_DEBUG game={} line={} player={} yellow_bank={} workers_free={} culture={}",
                    meta.id, line.lineno, pi, p.yellow_bank, p.workers_free, p.culture
                );
            }
        }
        if std::env::var("WARTIME_DEBUG").is_ok()
            && line.text.contains("declares War over")
        {
            for pi in 0..r.state.num_players {
                let p = &r.state.players[pi as usize];
                let st = crate::effects::state_stats(&r.state, p);
                let army = crate::effects::army_strength(p);
                eprintln!(
                    "WARTIME_DEBUG game={} line={} player={} total={} army={} base={} leader={:?}",
                    meta.id,
                    line.lineno,
                    pi,
                    st.strength,
                    army,
                    st.strength - army,
                    p.leader.get().name
                );
            }
        }
        if std::env::var("SCOREDIV_LINE_DEBUG").is_ok() {
            eprintln!(
                "SCOREDIV_LINE i={i} lineno={} game_over={} completed={} text={:.60?}",
                line.lineno, r.state.game_over, completed, line.text
            );
        }
        // Catch this reconstruction's `state.age_civil` up to what the
        // journal's own age column already proves happened -- see
        // `catch_up_civil_age`'s own doc for why this must be deferred
        // while an earlier line's work is still outstanding, and
        // `is_trustworthy_age_line`'s doc for why an untrustworthy line
        // (BGO's "Last turn" trailer, or an `EndTurn`/`Discard` wrap-up) must
        // not be allowed to force the age at all, even with pending/queue
        // both empty: game `7522064`'s "Last turn" sits ahead of a
        // DIFFERENT, still-old-age player's own fully-synchronous `End turn`.
        if is_trustworthy_age_line(classify(card_index, line.text)) {
            catch_up_civil_age(&mut r.state, line.age);
        }
        // Napoleon Bonaparte / War-over-Culture fix: this line's own
        // processing can synchronously trigger `game::start_turn`'s war
        // resolution one age before the confirming `WinWar` line is ever
        // reached (see `upcoming_confirmed_winwar_age`'s own doc). Run ONLY
        // the leader/wonder/pact half of antiquation early when that is
        // confirmed safe -- `age_civil`/`civil_deck`/§12.2.4 stay on the
        // normal, deferred schedule above.
        if let Some(age) = upcoming_confirmed_winwar_age(journal, i, card_index) {
            game::antiquate_leader_wonder_pacts_up_to(&mut r.state, age);
        }
        // Keep `civil_deck` from ever running dry ON ITS OWN -- see
        // `top_up_civil_deck`'s own doc. Must run every line, not just when
        // low: cheap (one length compare) when it's a no-op, which is
        // almost always.
        top_up_civil_deck(&mut r.state);
        // The checked half of the invariant `top_up_civil_deck` exists to
        // provide: turns "the floor always holds" from a hope into
        // something that fails LOUDLY (`difftest`/`cargo test` both run
        // with `debug-assertions = true`, `Cargo.toml`) the moment a future
        // change reopens the two-mechanisms-fighting bug this closes,
        // instead of silently reintroducing it. Safe under `panic = "abort"`
        // (`Cargo.toml`'s own note on why a stray `debug_assert` on a
        // diagnostic path is dangerous): this one is a direct consequence of
        // the line just above it, not a speculative check on unrelated
        // state, so it is exactly as reliable as `top_up_civil_deck` itself.
        debug_assert!(
            r.state.age_civil == crate::cards::Age::IV || r.state.civil_deck.len() >= CIVIL_DECK_SAFETY_FLOOR,
            "top_up_civil_deck left civil_deck under its own floor ({} < {CIVIL_DECK_SAFETY_FLOOR}) at line {}",
            r.state.civil_deck.len(),
            line.lineno
        );
        // Structural divergence instrument (docs/REPLAY.md "civil deck
        // model" handoff), kept on permanently rather than removed once the
        // fix landed: `catch_up_civil_age` above only ever moves
        // `age_civil` UP TO this line's own stated age, never past it, so
        // reading STRICTLY AHEAD here can only mean the PRIMARY, deck-
        // empty-triggered `advance_age` inside `game::deal` fired early --
        // and `last_line_index_for_age` rules out the harmless case of this
        // simply being the age's own true last line (nothing left in the
        // journal still tagged the old age). Recorded once, first
        // occurrence only.
        if civil_deck_premature_advance.is_none() {
            if let Some(claimed) = parse_age(line.age) {
                if r.state.age_civil > claimed {
                    if let Some(last) = last_line_index_for_age[claimed as usize] {
                        if i < last {
                            civil_deck_premature_advance = Some(PrematureCivilAdvance {
                                lineno: line.lineno,
                                journal_age: claimed,
                                reconstructed_age: r.state.age_civil,
                            });
                        }
                    }
                }
            }
        }
        // REPLAYER BUG: BGO's own journal states §12.3's "Age IV began ->
        // this round or the next is last" fact in-band ("Last turn Game ends
        // at the end of the starting round", one line per surviving player,
        // no leading actor colour -- `corpus::classify` resolves it to pure
        // `LineOutcome::Bookkeeping`, same as every other flavour line). This
        // binary used to just drop it. That is normally harmless -- the real
        // trigger is `game::advance_age` emptying `state.civil_deck` -- but
        // THIS binary's civil deck never empties on a real journal: row
        // slots are forced to match each observed "takes ... in hand" line
        // directly (`ground_row_slot`), not drawn through `civil_deck`, so
        // an entire game can replay with every human move legal and this
        // reconstruction's Age III deck still nonempty. `game::set_last_round`
        // (now `pub(crate)` for this one call) is idempotent -- guarded by
        // `state.final_round_end.is_some()` exactly like its `advance_age`
        // call site -- so calling it here from the journal's own authoritative
        // statement of the SAME fact, using this reconstruction's own
        // (up to here, accurate) `current`/`round`/`start_player`, sets
        // exactly what the natural deck-driven trigger would have, had it
        // fired. Without this, `state.final_round_end` stays `None` forever on
        // every sampled completion and `game::finish_game` -- reachable only
        // through `advance_turn`'s `round > final_round_end` check -- can
        // never run, so `state.game_over` never flips even on a fully legal
        // replay to the journal's own end.
        if line.text.starts_with("Last turn") {
            game::set_last_round(&mut r.state);
            continue;
        }
        // REPLAYER BUG (was): this used to `break` here, so nothing past
        // "End of game" was ever replayed. But BGO logs that marker BEFORE
        // the true last turn's own end-of-turn processing -- the trailing
        // "Impact of <Event>" final-scoring lines and the actual "End turn
        // <Color> scores: ..." line for the player who triggered the game
        // end all come AFTER it (`docs/REPLAY.md`, final-score cross-check
        // section). Breaking here meant `game::advance_turn`'s own
        // round-wrap check -- the only thing that ever calls
        // `game::finish_game` and flips `state.game_over` -- was never
        // reached even on a fully-legal replay, so a completed game was
        // undetectable except by this flag counting the journal's OWN
        // marker line, independent of engine state (see `GameResult::
        // engine_scores`). Now `completed` is set here but the loop keeps
        // going: `classify` already resolves "End of game" and every
        // "Impact of ..." line (they reprint the scoring card's own name as
        // subject) to `LineOutcome::Bookkeeping`, and the real trailing "End
        // turn" line takes the ordinary no-leading-colour `EndTurn` path
        // below, exactly like every other turn's end -- so `finish_game`
        // fires from the SAME code every mid-game end-turn already uses, not
        // a special case. BGO also logs that final "End turn" line and its
        // "No Discard Phase"/"discards N cards" follow-up TWICE, once
        // mislabelled a round ahead (identical score deltas both times,
        // confirmed across all sampled completions) -- the top-of-loop
        // `state.game_over` check just below is what stops the second copy
        // from ever being attempted against an already-finished game.
        if line.text.starts_with("End of game") {
            completed = true;
            // Ground the still-pending event piles from this line's own
            // real final-event SET before the trailing "End turn" line's
            // `finish_game` call reads them -- see `ground_final_events`.
            let real_final_events = parse_real_final_events(line.text, card_index);
            if std::env::var("SCOREDIV_EVENT_DEBUG").is_ok() {
                eprintln!(
                    "SCOREDIV_GROUND lineno={} game_over_already={} real_cards={:?}",
                    line.lineno,
                    r.state.game_over,
                    real_final_events.iter().map(|c| c.name()).collect::<Vec<_>>()
                );
            }
            if r.state.game_over {
                // `finish_game` already fired (via `advance_turn`'s round-wrap
                // check) against the FICTIONAL event piles this reconstruction
                // had in place -- that wrong award is already in `p.culture`.
                // Undo it off the current (still-fictional) piles, then ground
                // and re-apply the real award. Mirrors the TruncatedAfterGameOver
                // correction a few lines down; the only difference is that here
                // we are sitting ON the "End of game" line itself rather than
                // having to peek for it.
                for (_card, steps) in crate::events::final_event_awards(&r.state) {
                    for (idx, amount) in steps {
                        if amount != 0 {
                            let p = &mut r.state.players[idx as usize];
                            p.culture = (p.culture as i32 - amount).max(0) as u16;
                        }
                    }
                }
            }
            ground_final_events(&mut r.state, &real_final_events);
            if r.state.game_over {
                crate::events::evaluate_final_events(&mut r.state);
            }
            continue;
        }
        // Once the engine's own `finish_game` has fired, nothing after it
        // is a real decision -- it is BGO's duplicated/trailing tail (see
        // above). Stop reading rather than feed a finished game a move it
        // will legally reject.
        if r.state.game_over {
            // TruncatedAfterGameOver (GAMEDROP.txt, 68/789 skipped games,
            // 100% of a corpus-wide bucket, zero exceptions in either
            // direction): a MINORITY of journals log the duplicated
            // trailing "End turn" pair BEFORE their own "End of game" line
            // instead of after -- so the FIRST copy's `finish_game` call
            // (via `game::advance_turn`'s round-wrap check) already fired,
            // `game_over` is already true, and this guard trips before the
            // marker two lines above is ever reached. `completed` staying
            // false there is correct: it is set ONLY by literally reading
            // that line's text, and nothing has read it yet. Recover here,
            // rather than widen the branch above, so every currently-
            // completing game (`completed` already true by construction:
            // it can only become true by having ALREADY read "End of game")
            // takes zero-instruction-different code through this arm --
            // this whole block is dead for them.
            if !completed {
                // Peek at the still-unread remainder for the same marker
                // the branch above reads, exactly as it is for every other
                // game (`docs/REPLAY.md`/GAMEDROP.txt: always present,
                // always within 1-2 lines, 68/68 measured). Not found means
                // a genuinely different truncation (e.g. a resignation) --
                // leave `completed` false and reject, same as today.
                if let Some(eog_line) = journal[i..].iter().find(|l| l.text.starts_with("End of game")) {
                    // `finish_game` already ran `events::evaluate_final_events`
                    // against the FICTIONAL piles this reconstruction had
                    // filled in (`event_plan`'s own doc: "filled with
                    // unused cards of the right age and kind", never
                    // validated for final scoring) -- that wrong award is
                    // already added into `p.culture` and must be undone
                    // before the real, grounded award (the SAME function,
                    // same call the branch above already makes) can be
                    // applied in its place. Nothing has touched
                    // `current_events`/`future_events` or any player's
                    // culture since `finish_game` returned (the lines
                    // between are exactly the duplicated trailer this
                    // module's own doc above describes), so recomputing
                    // the wrong award off the CURRENT (still fictional)
                    // piles reproduces exactly what was just added.
                    for (_card, steps) in crate::events::final_event_awards(&r.state) {
                        for (idx, amount) in steps {
                            if amount != 0 {
                                let p = &mut r.state.players[idx as usize];
                                p.culture = (p.culture as i32 - amount).max(0) as u16;
                            }
                        }
                    }
                    let real_final_events = parse_real_final_events(eog_line.text, card_index);
                    ground_final_events(&mut r.state, &real_final_events);
                    crate::events::evaluate_final_events(&mut r.state);
                    completed = true;
                }
            }
            break;
        }
        if putback_skips.contains(&i) {
            continue;
        }
        // Already applied early, out of journal order, by `resolve_
        // intervening`'s `ChoiceKind::LosePop` handling (`claimed_destroy_
        // lines`'s own doc) -- translating it again here would double-apply
        // the same destroy.
        if r.claimed_destroy_lines.contains(&i) {
            continue;
        }
        r.current_lineno = line.lineno;
        crate::tie_context::set_lineno(line.lineno);
        // Self-deferring: a no-op unless a prior `EndTurn` line's culture
        // checkpoint is still waiting on a discard decision to resolve --
        // see `PendingCultureCheck`'s own doc.
        r.flush_pending_culture_check();
        // Science/resources oracle twins -- no-ops (an `Option::take` on
        // `None`) unless `SCIENCE_ORACLE`/`RESOURCE_ORACLE` is set, since
        // `pending_science_check`/`pending_resource_check` are only ever
        // populated by the gated capture sites below.
        r.flush_pending_science_check();
        r.flush_pending_resource_check();
        // Self-deferring the same way: a no-op unless an earlier `"GAME
        // DATA UPDATED"` culture clause is still waiting for this
        // reconstruction's own live value to catch up -- see `Replayer::
        // pending_culture_corrections`'s own doc.
        r.flush_pending_culture_corrections();
        // Same self-deferring flush, `Bronze`/resources side -- see
        // `Replayer::pending_resource_corrections`'s own doc.
        r.flush_pending_resource_corrections();
        // Same self-deferring flush, `science` side -- see
        // `Replayer::pending_science_corrections`'s own doc.
        r.flush_pending_science_corrections();
        // BGO's own admin correction (`parse_game_data_updated_culture`'s
        // own doc: rare, real, and previously silently dropped as
        // `Bookkeeping`) -- apply every culture clause it carries directly
        // to `state.players[_].culture` before anything else touches this
        // line. No `Move` models this (there is no player action to
        // replay), so it cannot go through `classify`'s ordinary
        // `ActionClass` dispatch below -- checked here, first, same as the
        // "Last turn"/"End of game" special lines just above. Only applied
        // when the clause's own stated `old` agrees with this
        // reconstruction's live value for that player RIGHT NOW; otherwise
        // queued onto `pending_culture_corrections` and retried on later
        // lines (see that field's own doc for why: BGO's `old` and our live
        // value can be answering different questions mid-deferred-cascade,
        // and applying a mismatched delta there would corrupt a checkpoint
        // that already agreed rather than fix one that didn't).
        //
        // `resource_corrections` is the identical treatment for the
        // `Bronze` (resources) clause the same line can carry (`parse_
        // game_data_updated_resources`'s own doc) -- checked on the SAME
        // line, not an `else if`, because a real sampled line carries BOTH
        // a `science`/`Bronze` clause together (`science: 2 -> 5; Bronze: 0
        // -> 2`); `science_corrections` is the third, identical treatment
        // (`parse_game_data_updated_science`'s own doc) -- checked on the
        // SAME line too, since a real sampled line (game `7522617` line
        // 105) carries `science` AND `Bronze` clauses together (`science: 7
        // -> 8; Bronze: 4 -> 5`). Any clause kind alone is enough to skip
        // `classify` below, since this line is BGO bookkeeping either way,
        // never a `Move`.
        let culture_corrections = parse_game_data_updated_culture(line.text);
        let resource_corrections = parse_game_data_updated_resources(line.text);
        let science_corrections = parse_game_data_updated_science(line.text);
        if !culture_corrections.is_empty() || !resource_corrections.is_empty() || !science_corrections.is_empty() {
            for (color, old, new) in culture_corrections {
                let p = &mut r.state.players[color.seat() as usize];
                if p.culture as i32 == old {
                    p.culture = new.max(0) as u16;
                } else {
                    r.pending_culture_corrections.push((color, old, new));
                }
            }
            for (color, old, new) in resource_corrections {
                let p = &mut r.state.players[color.seat() as usize];
                if p.resources as i32 == old {
                    p.resources = new.max(0) as u16;
                } else {
                    r.pending_resource_corrections.push((color, old, new));
                }
            }
            for (color, old, new) in science_corrections {
                let p = &mut r.state.players[color.seat() as usize];
                if p.science as i32 == old {
                    p.science = new.max(0) as u16;
                } else {
                    r.pending_science_corrections.push((color, old, new));
                }
            }
            continue;
        }
        let outcome = classify(card_index, line.text);
        let LineOutcome::Action(Classified { class, card }) = outcome else {
            continue; // bookkeeping / unclassified: no move to apply
        };
        // Captured BEFORE `r.last_action_class` is overwritten by this
        // line's own class below -- "the last classified action line's
        // class, of any actor, strictly before this line" is exactly what
        // the culture oracle's `last_action_class` field wants at its own
        // `EndTurn` checkpoint (see `CultureOracleDivergence`'s doc). Every
        // classified line hits this unconditionally, so no `continue`
        // branch elsewhere in the loop needs to remember to set it.
        let previous_action_class = r.last_action_class.replace(class);
        let Some((actor_color, rest)) = actor_and_rest(line.text) else {
            // EndTurn lines start with "End turn", no leading colour --
            // the actor is whoever the engine currently has as `current`.
            if class == ActionClass::EndTurn {
                let actor = r.state.current;
                r.auto_passed[actor as usize] = 0;
                // New turn starting for `actor`: the running Take-spend sum
                // above belongs to the turn that just ended. BGO logs this
                // "End turn" line twice for the true final turn (this
                // function's own comment a few lines up); zeroing twice is
                // a no-op the second time.
                ca_take_spend_this_turn[actor as usize] = 0;
                // `resolve_intervening` can itself finish the game (draining
                // the true final turn's last queued discard resumes
                // `game::resume_end_turn`, which can run `finish_game` -- see
                // that function's own new doc). BGO logs this exact "End
                // turn" line TWICE; applying `Move::EndTurn` a second time
                // against an already-`game_over` state is not a real human
                // action to replay, just this file catching up to a finish
                // that already happened -- `legal_moves` is empty once
                // `game_over` (`legal.rs`), so trying anyway would report a
                // bogus `IllegalMove` for what is actually a clean end.
                if let Err(kind) = r.resolve_intervening(actor, (class, None), false).and_then(|()| {
                    if r.state.game_over {
                        Ok(())
                    } else {
                        // Right here, `self.state.players[actor].hand_military`
                        // is EXACTLY what `interact::discard_excess_military`
                        // (the very next thing `try_apply` below runs, as step
                        // 1 of `economy::end_of_turn`) is about to read -- the
                        // one checkpoint in the whole file where this binary's
                        // own reconstructed military-hand size can be compared
                        // against BGO's own stated truth for free, before a
                        // drift becomes a `StuckPending` several rounds later.
                        // See [`Replayer::check_discard_phase_oracle`]'s own
                        // doc and the module doc's "Discard-phase hand-size
                        // oracle" section. Skipped on the `game_over` branch
                        // above (BGO's documented duplicate trailing "End
                        // turn" line for the true final turn) -- nothing left
                        // to check against a state that already finished.
                        r.check_discard_phase_oracle(actor, line);
                        r.record_corruption_check(actor, line);
                        apply_churchill_end_turn_choice(&mut r, line.text)?;
                        r.try_apply(Move::EndTurn, true)
                    }
                }) {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
                }
                // Investigation aid for the `IllegalMove: Develop`/
                // `PlayAction` buckets (both "science payment" shaped):
                // BGO's own end-turn line prints the banked-science RUNNING
                // TOTAL (`trailing_now_science`'s doc), which is ground
                // truth this binary can check itself against for free,
                // every single turn, not just at the eventual spend that
                // finally trips over a shortfall many lines later. NOTE:
                // this fires BEFORE a deferred `resume_end_turn` (a queued
                // military discard) has actually run -- a discard-blocked
                // turn reads as a false "drift" here that resolves itself a
                // few lines later; cross-check against the LATER
                // `free_civil_action_move`/`try_apply` science reading
                // before trusting a single one of these in isolation.
                if crate::debugflags::replay_debug() {
                    if let Some(want) = trailing_now_science(line.text) {
                        let got = r.state.players[actor as usize].science as i32;
                        if want != got {
                            eprintln!(
                                "DEBUG end-turn science drift: actor={actor} journal says (now {want}), \
                                 this binary computes {got} (delta {}) at {:?}",
                                got - want,
                                line.text,
                            );
                        }
                    }
                    if let Some(want) = trailing_now_resources(line.text) {
                        let got = r.state.players[actor as usize].resources as i32;
                        let discard_blocked =
                            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary);
                        if want != got && !discard_blocked {
                            eprintln!(
                                "DEBUG end-turn resources drift: actor={actor} journal says (now {want}), \
                                 this binary computes {got} (delta {}) at {:?}",
                                got - want,
                                line.text,
                            );
                        }
                    }
                }
                // Structural, always-on culture-oracle checkpoint -- see
                // `CultureOracleDivergence`'s own doc. Unlike the science
                // drift check above (still `REPLAY_DEBUG`-gated eyeball
                // tooling for a different bucket's investigation), this is
                // the task's own deliverable: BGO's "(now M)" running total
                // is a perfect, always-present oracle for the exact
                // per-player number this project is currently -6.36 mean
                // wrong on, cross-validated every single "End turn" line,
                // not derived or approximated.
                if let Some(want) = trailing_now_culture(line.text) {
                    // `economy::end_of_turn` stops BEFORE running production
                    // (steps 2-5) when it opens a discard decision -- see
                    // `PendingCultureCheck`'s own doc for why comparing
                    // `r.state.players[actor].culture` right here, in that
                    // case, is a false positive: defer to the next line's
                    // dispatch instead, by which point the discard (and the
                    // production it was blocking) has actually run.
                    if matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary)
                    {
                        r.pending_culture_check = Some(PendingCultureCheck {
                            lineno: line.lineno,
                            actor_seat: actor,
                            journal_now: want,
                            last_action_class: previous_action_class,
                        });
                    } else {
                        r.record_culture_check(line.lineno, actor, want, previous_action_class);
                    }
                }
                // Science/resources oracle twins of the culture checkpoint
                // just above -- SAME "(now M)" running-total grammar, SAME
                // line, SAME discard-blocked-turn deferral shape, but gated
                // behind their own `SCIENCE_ORACLE`/`RESOURCE_ORACLE` env
                // vars (2026-08-14 pass: "DIAGNOSTIC-ONLY code: it must not
                // change any replay outcome" -- unlike culture's always-on
                // checkpoint, these must default to fully inert). See
                // `ScienceOracleDivergence`/`ResourceOracleDivergence`'s own
                // docs.
                if crate::debugflags::science_oracle() {
                    if let Some(want) = trailing_now_science(line.text) {
                        if matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary) {
                            r.pending_science_check = Some(PendingScienceCheck {
                                lineno: line.lineno,
                                round: line.round.to_string(),
                                actor_seat: actor,
                                journal_now: want,
                                last_action_class: previous_action_class,
                            });
                        } else {
                            r.record_science_check(line.lineno, line.round, actor, want, previous_action_class);
                        }
                    }
                }
                if crate::debugflags::resource_oracle() {
                    if let Some(want) = trailing_now_resources(line.text) {
                        if matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.kind == ChoiceKind::DiscardMilitary) {
                            r.pending_resource_check = Some(PendingResourceCheck {
                                lineno: line.lineno,
                                round: line.round.to_string(),
                                actor_seat: actor,
                                journal_now: want,
                                last_action_class: previous_action_class,
                            });
                        } else {
                            r.record_resource_check(line.lineno, line.round, actor, want, previous_action_class);
                        }
                    }
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
            // Christopher Columbus's leader ability, the one line in the
            // whole corpus with NEITHER a leading colour NOR a trailing
            // consequence clause naming the actor (`corpus::ActionClass::
            // ColumbusColonize`'s own doc) -- column 2 (`Line::color`) is
            // the only place the actor is. Also a political action, so
            // `next_line_explains_own_politics: true`, same reasoning as
            // `RemoveLeaderYellow` just above.
            if class == ActionClass::ColumbusColonize {
                let Some(actor_color) = Color::parse(line.color) else {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("column 2 {:?} is not a known colour", line.color))));
                    break 'lines;
                };
                let actor = actor_color.seat();
                if actor >= meta.players {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("actor colour {actor_color:?} outside {}p seating", meta.players))));
                    break 'lines;
                }
                let Some(territory) = card else {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("Columbus discovery line's territory did not resolve to a known card".into())));
                    break 'lines;
                };
                // This "discovers" line is routinely the FIRST evidence of
                // which real territory sits in the actor's military hand --
                // territories arrive via the automatic end-of-turn draw, not
                // an observed "takes ... in hand" line, so `p.hand_military`
                // still holds `new_game`'s SIMULATED filler for that slot
                // until grounded here, same as `DeclareWar`/`PlayAggression`/
                // `ProposePact` already ground their own military card right
                // before playing it (`Replayer::consume_named_military_card`).
                // This call site cannot USE that atomic wrapper, though: an
                // unavoidable `resolve_intervening` step sits between the
                // ground and the consuming `Move::ColumbusColonize` below
                // (auto-resolving any pending decision the grounding itself
                // might have unblocked), so it calls the lower-level
                // `ground_for_consumption` primitive directly -- one of the
                // two doc'd exceptions that primitive's own comment names.
                r.ground_for_consumption(actor, territory);
                if let Err(kind) = r
                    .resolve_intervening(actor, (class, None), true)
                    .and_then(|()| r.try_apply(Move::ColumbusColonize { card: territory }, true))
                {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
                }
                r.actions_consumed += 1;
                continue;
            }
            // International Agreement (Code of Laws p.12), also no leading
            // colour on BGO's own first-pick line -- `corpus::classify`'s
            // own doc comment on this shape. Column 2 (`Line::color`) names
            // the taking civilization reliably (every sampled game: it
            // always matches the colour embedded in the take clause
            // itself), the same source `ColumbusColonize` just above
            // already trusts for the same reason. Not a political action of
            // the taker's own (the event interrupts WHOEVER played it, not
            // the taker's own political phase), so
            // `next_line_explains_own_politics: false`, same as
            // `Barbarossa` below.
            //
            // Unlike every other shape this file replays (one action per
            // JOURNAL LINE), BGO logs EVERY pick the civilization makes on
            // ONE line, ";"-joined ("International Agreement <Color> takes
            // <A> in hand; <Color> takes <B> in hand; ..."). `corpus::
            // classify` only ever resolves the FIRST pick (`<A>`, `card`
            // below); this loop applies it, then walks the line's own
            // remaining ";"-clauses for any further picks, applying each
            // the same way.
            //
            // Each pick is translated into a `Move::Choose` naming the open
            // `Pending::Choice(TakeRow)`'s matching `Slot` option directly
            // (rather than routed through `apply_one`'s generic `TakeCard`
            // arm) because that arm prices the take via `observed_take_cost`
            // -- BGO's `"uses N civil action"` clause -- which International
            // Agreement's own picks NEVER carry (paid from the event's own
            // budget, not the player's civil-action pool; `interact.rs`'s
            // `ChoiceKind::TakeRow` handler bills `budget`, not
            // `p.civil_actions`). Feeding that missing clause's default (0)
            // into `ground_row_slot`'s cost-matching search would demand a
            // slot whose ORDINARY position price happens to be exactly 0,
            // which is true for at most one row slot and wrong for every
            // other real pick -- so this calls `ground_row_slot` with
            // `None` (its own forgiving "already grounded, else first
            // ungrounded slot" fallback) instead.
            if class == ActionClass::TakeCard && line.text.starts_with("International Agreement ") {
                let Some(actor_color) = Color::parse(line.color) else {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("column 2 {:?} is not a known colour", line.color))));
                    break 'lines;
                };
                let actor = actor_color.seat();
                if actor >= meta.players {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("actor colour {actor_color:?} outside {}p seating", meta.players))));
                    break 'lines;
                }
                let Some(first_pick) = card else {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("International Agreement take line's card did not resolve to a known card".into())));
                    break 'lines;
                };
                // Re-resolved to the age copy actually in the row, same as
                // `apply_one`'s ordinary `TakeCard` arm does for every other
                // take (`best_age_sibling`'s own doc comment on the nine
                // card families BGO's text never tags with an age --
                // `Reserves` among them). Skipping this left `first_pick`/
                // `picks` pointing at `corpus::build_card_index`'s arbitrary
                // same-name pick, which then matched NO row slot at all
                // (wrong age = different `CardId`) -- `resolve_intervening`'s
                // own `matches_upcoming` read that as a silent decline and
                // drained the just-opened `TakeRow` via `Stop`, corpus-
                // confirmed as this bucket's own two regressions
                // (`StuckPending`/`ParserGap: ... no open TakeRow choice`
                // for `Reserves`/`First Space Flight`, games `7522957`/
                // `7523114`).
                let first_pick = best_age_sibling(first_pick, r.state.age_civil);
                let mut picks = vec![first_pick];
                for clause in line.text.split("; ").skip(1) {
                    let Some(take_rest) =
                        clause.strip_prefix(actor_color.as_str()).and_then(|r| r.strip_prefix(' ')).and_then(|r| r.strip_prefix("takes "))
                    else {
                        break;
                    };
                    let Some(card_end) = take_rest.find(" in hand") else { break };
                    let Some((id, _)) = longest_known_card_prefix(card_index, &take_rest[..card_end]) else { break };
                    picks.push(best_age_sibling(id, r.state.age_civil));
                }
                // `resolve_intervening` is called ONCE, for the FIRST pick
                // only -- it drains the `QueueItem::TakeRow` the event's own
                // resolution enqueued (`events.rs`'s `optional_take_cards_
                // with_civil_actions` arm), which is what actually
                // materialises the `Pending::Choice(TakeRow)` this loop reads
                // below. Every LATER pick's own re-offer is already
                // synchronous (`interact.rs`'s `ChoiceKind::TakeRow` match
                // arm calls `offer_take_row` inline, not through the queue),
                // so calling it again per pick was a bug in its own right,
                // independent of the slot-matching one above: with no
                // ungrounded slot of THIS card's identity anywhere yet (it
                // is not grounded until the loop body below places it), its
                // own `matches_upcoming` check always reads false and drains
                // the just-reopened choice via `Stop` -- corpus-confirmed
                // (`ParserGap: International Agreement pick with no open
                // TakeRow choice`, 42/1,011 games with the per-pick call).
                if let Err(kind) = r.resolve_intervening(actor, (ActionClass::TakeCard, Some(first_pick)), false) {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
                }
                for pick in picks {
                    let Some(Pending::Choice(c)) = r.state.pending.top() else {
                        mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("International Agreement pick with no open TakeRow choice".into())));
                        break 'lines;
                    };
                    if !matches!(c.kind, ChoiceKind::TakeRow { .. }) {
                        mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("expected an open TakeRow choice, found {:?}", c.kind))));
                        break 'lines;
                    }
                    // Ground `pick` against one of THIS CHOICE'S OWN offered
                    // `Slot` options, not `ground_row_slot`'s generic "first
                    // ungrounded slot anywhere" fallback -- `offer_take_row`
                    // (`interact.rs`) filters its `opts` down to slots that
                    // are both affordable within the event's own budget AND
                    // `costs::can_take`-legal, a STRICT subset of all 13 row
                    // slots. Forcing `pick` into an arbitrary ungrounded slot
                    // outside that subset (the bug this replaced) reliably
                    // named a slot the choice never offered at all --
                    // `ParserGap: open TakeRow choice does not offer slot #`,
                    // 14/1,011 games once `corpus::classify` started
                    // resolving these lines instead of dropping them. Prefers
                    // a slot ALREADY grounded to `pick` (by identity, the
                    // same coincidence `ground_row_slot`'s own first branch
                    // guards against trusting blindly) over forcing a fresh
                    // one, for the same reason that branch exists there.
                    let options: Vec<ChoiceOption> = c.options.as_slice().to_vec();
                    let mut chosen: Option<(u8, usize)> = None;
                    for (oi, opt) in options.iter().enumerate() {
                        if let ChoiceOption::Slot(s) = opt {
                            if r.row_grounded[*s as usize] && r.state.card_row[*s as usize] == pick {
                                chosen = Some((*s, oi));
                                break;
                            }
                        }
                    }
                    if chosen.is_none() {
                        for (oi, opt) in options.iter().enumerate() {
                            if let ChoiceOption::Slot(s) = opt {
                                if !r.row_grounded[*s as usize] {
                                    chosen = Some((*s, oi));
                                    break;
                                }
                            }
                        }
                    }
                    let Some((slot, n)) = chosen else {
                        mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("open TakeRow choice offers no ungrounded slot for {}", pick.get().name))));
                        break 'lines;
                    };
                    r.state.card_row[slot as usize] = pick;
                    r.row_grounded[slot as usize] = true;
                    if let Err(kind) = r.try_apply(Move::Choose { n: n as u8 }, true) {
                        mismatch = Some(mk_mismatch(line, kind));
                        break 'lines;
                    }
                    r.row_grounded[slot as usize] = false;
                    r.actions_consumed += 1;
                }
                continue;
            }
            // Frederick Barbarossa's leader ability, also no leading colour
            // -- the actor is named only in the trailing "<Color> spends
            // ..." clause(s) (`corpus::classify`'s own comment on
            // `ActionClass::Barbarossa`). Unlike Alexander's death line,
            // this is an ACTION-PHASE action (the card text: "as an action-
            // phase action, spend 1 military action to..."), not a
            // political one, so `next_line_explains_own_politics: false` --
            // the same as every ordinary Build/Take/Upgrade line reached
            // through the normal leading-colour path below.
            if class == ActionClass::Barbarossa {
                let card = card.ok_or_else(|| MismatchKind::ParserGap("Barbarossa enlists with no resolved card".into()));
                let card = match card {
                    Ok(card) => card,
                    Err(kind) => {
                        mismatch = Some(mk_mismatch(line, kind));
                        break 'lines;
                    }
                };
                let Some(actor_color) = color_after(line.text, "; ") else {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("Barbarossa enlist line missing its trailing actor colour".into())));
                    break 'lines;
                };
                let actor = actor_color.seat();
                if actor >= meta.players {
                    mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("actor colour {actor_color:?} outside {}p seating", meta.players))));
                    break 'lines;
                }
                if let Err(kind) = r
                    .resolve_intervening(actor, (class, Some(card)), false)
                    .and_then(|()| r.try_apply(Move::Barbarossa { card }, true))
                {
                    mismatch = Some(mk_mismatch(line, kind));
                    break 'lines;
                }
                r.actions_consumed += 1;
                continue;
            }
            // J. S. Bach's leader ability, also no leading colour -- unlike
            // Barbarossa/Alexander this needs no trailing-clause colour scan
            // at all: an action-phase action can only ever be the CURRENT
            // actor's own move (`corpus::classify`'s own comment on
            // `ActionClass::BachTheater`), the same reasoning `EndTurn`
            // already relies on just above. Routed through the shared
            // `apply_one` (rather than inlined like `Barbarossa`) because
            // its own `BachTheater` arm already needs the full "<From> to
            // <To> ... [using <Card>]" parsing `rest` supplies, and
            // duplicating that here would be a second copy of the same
            // conditional this project's style rules single out.
            if class == ActionClass::BachTheater {
                let actor = r.state.current;
                // `classify` only ever returns `BachTheater` for a line
                // that starts with this exact prefix (its own doc comment),
                // so this can't fail.
                let rest = line.text.strip_prefix("Johannes Sebastian Bach").unwrap_or(line.text);
                let explains_own_politics = false;
                if !is_pure_confirmation_line(class) {
                    if let Err(kind) = r.resolve_intervening(actor, (class, card), explains_own_politics) {
                        mismatch = Some(mk_mismatch(line, kind));
                        break 'lines;
                    }
                }
                let next_text = journal.get(i + 1).map(|l| l.text);
                match apply_one(&mut r, actor, class, card, rest, line.text, next_text) {
                    Ok(()) => {
                        r.actions_consumed += 1;
                        continue;
                    }
                    Err(kind) => {
                        mismatch = Some(mk_mismatch(line, kind));
                        break 'lines;
                    }
                }
            }
            mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap("action line has no leading colour and is not EndTurn".into())));
            break 'lines;
        };
        let actor = actor_color.seat();
        if actor >= meta.players {
            mismatch = Some(mk_mismatch(line, MismatchKind::ParserGap(format!("actor colour {actor_color:?} outside {}p seating", meta.players))));
            break 'lines;
        }

        // Civil-action-TOTAL undercount check (docs/REPLAY.md "civil action
        // total" handoff). Every `TakeCard` line carries BGO's own explicit
        // `"uses N civil action"` clause, and a card is NEVER taken for
        // free: `legal::free_action_moves`'s `FreeActionKind` enum has no
        // Take variant, and `civil_life_move` (this file, above) only ever
        // offers Pop/Build/Develop for Civil Life's one-time discount --
        // grepped both exhaustively. So this `N` is unconditional ground
        // truth for civil actions this ACTUAL human spent, independent of
        // anything this reconstruction computes. Summed since `actor`'s own
        // last `EndTurn` (reset above), it is a hard LOWER BOUND on their
        // true civil-action total for the turn: if it ever exceeds
        // `costs::ca_total` -- what THIS reconstruction currently believes
        // that total is, using the exact function `costs::civil_hand_limit`
        // (the HandFull gate) is built from -- the total itself is
        // undercounted, independent of and prior to any other gate. Placed
        // BEFORE `resolve_intervening`/`apply_one` so it fires even on the
        // one Take line that is about to be rejected (a `HandFull` stop),
        // which is exactly the case this check exists to catch.
        if class == ActionClass::TakeCard {
            let (civil_clause, military_clause) = civil_and_military_uses(line.text);
            if let Some(civil_n) = civil_clause {
                // Net out Hammurabi's once-per-turn MA-for-CA conversion
                // (see `civil_and_military_uses`'s own doc): a trailing
                // "... uses N military action" clause on THIS SAME take
                // line means N of the printed civil price was paid from the
                // military pool instead, so the true civil-pool draw is
                // less than the printed price by that amount. Floored at 0,
                // never negative -- the two clauses are never expected to
                // put the military amount above the civil one.
                let cost = (civil_n - military_clause.unwrap_or(0)).max(0);
                ca_take_spend_this_turn[actor as usize] += cost;
                let spend = ca_take_spend_this_turn[actor as usize];
                let total_now = costs::ca_total(&r.state, &r.state.players[actor as usize]);
                if crate::debugflags::replay_debug() {
                    let gov = &r.state.players[actor as usize].government;
                    eprintln!(
                        "CA_TOTAL_CHECK game={} actor={actor} lineno={} age_civil={:?} round={} \
                         take_spend_this_turn={spend} ca_total={total_now} margin={} gov={}",
                        meta.id,
                        line.lineno,
                        r.state.age_civil,
                        r.state.round,
                        total_now - spend,
                        if gov.is_none() { "none" } else { gov.get().name },
                    );
                }
                if spend > total_now {
                    eprintln!(
                        "CA_TOTAL_UNDERCOUNT game={} actor={actor} lineno={} age_civil={:?} round={} \
                         take_spend_this_turn={spend} ca_total={total_now} deficit={}",
                        meta.id,
                        line.lineno,
                        r.state.age_civil,
                        r.state.round,
                        spend - total_now,
                    );
                }
            }
        } else if matches!(class, ActionClass::ElectLeader | ActionClass::PutBack) {
            // Net out the two in-turn refunds `trailing_gets_civil_action`'s
            // own doc names -- see that function for why these must NOT be
            // folded into the running Take-spend sum above.
            if let Some(refund) = trailing_gets_civil_action(line.text) {
                // NOT floored at 0: the refund can arrive before any
                // Take this turn (e.g. a leader replaced as the turn's
                // very FIRST action, before any card is taken) -- the
                // credit must still be there to net against a LATER
                // Take's cost. Clamping to 0 here silently threw the
                // credit away, which is exactly what produced 4 of this
                // check's own false-positive "undercounts" before this
                // fix (`docs/REPLAY.md` "civil action total" handoff):
                // the refund fired first with nothing yet to subtract
                // from, got floored to 0, and the credit vanished.
                let cur = &mut ca_take_spend_this_turn[actor as usize];
                if crate::debugflags::replay_debug() {
                    eprintln!(
                        "CA_REFUND game={} actor={actor} lineno={} class={class:?} refund={refund} before={} after={}",
                        meta.id, line.lineno, *cur, *cur - refund
                    );
                }
                *cur -= refund;
            }
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
        //
        // `ActionClass::WinWar` (`"X wins War over ... Attacker's strength:
        // N; Defender's strength: M"`) is the SAME shape again, for the SAME
        // reason, but the desync is bigger: `game::start_turn`'s own doc
        // says war resolution fires at the START OF THE ATTACKER'S NEXT
        // TURN, an engine-internal side effect with no journal line of its
        // own at all -- `apply_one`'s `WinWar` arm is already a bare `Ok(())`
        // "validation checkpoint only". BGO's `"wins War"` confirmation
        // names the WINNER (attacker or defender, whichever the strength
        // favoured), and can be timestamped identically to, and printed
        // immediately BEFORE, a completely unrelated other player's own
        // trailing `"End turn"` line, with no `EndTurn` in between --
        // confirmed on real game `7523809` line 342. Calling `resolve_
        // intervening` here sent `expected_actor` to the named winner while
        // the true decider was still mid a DIFFERENT player's turn, with no
        // pending open to explain the gap -- 59 of the 216 games in the
        // `StuckPending: decider != expected actor ... no pending` bucket
        // stopped on exactly this line shape (`docs/REPLAY.md`).
        // BGO logs the SAME end-of-turn discard TWICE, at the SAME
        // timestamp: the player's own `End turn ... draws N military
        // cards` line, then a trailing `<Color> discards N card(s)`
        // line. By the time the trailing line is reached the `EndTurn`
        // arm has already run `try_apply(Move::EndTurn)`, whose `economy::
        // end_of_turn` advanced `state.current` to the NEXT player -- so
        // the trailing line's `actor` (the PREVIOUS player) is no longer
        // `state.decider()`. The `DiscardMilitary` pending that `end_of_
        // turn` step 1 opened was ALREADY resolved by the `EndTurn` arm's
        // own `resolve_intervening` (the `matches_upcoming` deferral
        // drained it via `apply_one`'s `Discard` arm), so there is nothing
        // left for the trailing line to do: it is a pure confirmation, the
        // same shape as `PlayEvent`/`WinAuction`/`Colonize`/`WinWar`, and
        // calling `resolve_intervening` for it sends `expected_actor` to
        // the PREVIOUS player while `state.decider()` is already the NEXT
        // one -- a guaranteed `StuckPending: decider != expected actor,
        // phase Actions, no pending` for 89 of the ~116 games in that
        // bucket. The distinguishing signal is the ENGINE STATE itself,
        // not the journal text: a GENUINE end-of-turn discard (the
        // `Discard Phase` line, resolved by the same `EndTurn` arm's
        // `apply_one`) is reached while `state.current` is STILL the
        // acting player, so `decider() == actor` there and this check
        // never fires for it. Found on real game `7523354` line 251:
        // `"Purple discards 1 card"` whose `End turn Purple` line (250)
        // already advanced `state.current` to Orange (seat 0), so
        // `decider()==0 != actor==1`.
        if class == ActionClass::Discard && r.state.decider() != actor {
            r.trailing_discards_lines.push(line.lineno);
            r.actions_consumed += 1;
            continue 'lines;
        }

        // BGO's client logs a single real build as TWO identical
        // `"<Color> builds <Card>"` lines seconds apart (e.g. real game
        // `7522652` has FOUR "Orange builds Movies" lines -- r15, r16 twice,
        // r17 -- but BGO's own end-of-turn resource math proves only 3
        // happened, because the second r16 line, 38s after the first, is the
        // redundant confirmation; and `7522632` logs "Purple builds 1 stage
        // of Library of Alexandria" THREE times for a 1-stage wonder built
        // once). This file used to apply EVERY line, over-spending the build's
        // resources each time the journal duplicated it -- which then starved
        // a later, genuinely-unaffordable build into `IllegalMove:
        // WonderStep`/`Build` (the shared root cause of those two buckets).
        //
        // The skip mirrors `trailing_discards_lines` two lines up: a build
        // line is a pure confirmation -- skip it -- when BOTH
        //   (a) an IDENTICAL `"<Color> builds <Card>"` line for the SAME
        //       actor earlier in the journal was already applied (its
        //       `Move::Build{card}` is in `moves_applied`), AND
        //   (b) re-applying it is now ILLEGAL in the reconstructed state
        //       (`Move::Build{card}` absent from `legal::legal_moves`).
        // Condition (b) is the load-bearing one: two GENUINE, distinct builds
        // of the same card (a real 2x "builds Knights") both apply legally in
        // sequence, so neither ever satisfies (b) and neither is skipped --
        // only a re-application the engine itself now refuses is, which is
        // exactly the redundant double-log shape. A `FreeBuild` confirmation
        // (which this file already routes through `Move::Choose`, not
        // `Move::Build`) never reaches here: `moves_applied` holds no
        // `Move::Build{card}` for it. Gated on the journal's own age/round
        // columns (same turn) so it can only fire on a same-turn duplicate.
        if matches!(class, ActionClass::BuildBuilding | ActionClass::BuildUnit)
            && card.is_some_and(|c| {
                let prior_identical_build_line = journal.iter().take(i).any(|l| {
                    let same_line = l.round == line.round && l.age == line.age && l.text == line.text;
                    let same_build = matches!(
                        classify(card_index, l.text),
                        LineOutcome::Action(ref x)
                            if x.class == class && x.card == Some(c)
                    );
                    same_line && same_build
                });
                let already_applied =
                    r.moves_applied.iter().any(|mv| matches!(mv, Move::Build { card: d } if *d == c));
                let now_illegal =
                    !legal::legal_moves(&r.state).as_slice().contains(&Move::Build { card: c });
                prior_identical_build_line && already_applied && now_illegal
            })
        {
            r.duplicate_build_lines.push(line.lineno);
            r.actions_consumed += 1;
            continue 'lines;
        }

        if !is_pure_confirmation_line(class) {
            if let Err(kind) = r.resolve_intervening(actor, (class, card), explains_own_politics) {
                mismatch = Some(mk_mismatch(line, kind));
                break 'lines;
            }
        }

        // `resolve_intervening`'s civil-life re-check may have just applied
        // this line's action (a political action banked a fresh
        // `one_time_discount` after the main loop's own `civil_life_move`
        // check ran). If so, skip `apply_one` to avoid double-applying.
        if r.claimed_civil_life_lines.contains(&line.lineno) {
            r.actions_consumed += 1;
            continue 'lines;
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
    // `finish_game` does not clear `current_events`/`future_events` -- it
    // only reads them via `events::final_event_awards` -- so recomputing
    // that same call post-hoc reproduces exactly the SET this game's own
    // `engine_scores` were built from, with no separate snapshot needed.
    let final_event_awards_posthoc = if completed && r.state.game_over {
        Some(crate::events::final_event_awards(&r.state))
    } else {
        None
    };
    let final_event_cards =
        final_event_awards_posthoc.as_ref().map(|awards| awards.iter().map(|(c, _)| c.name()).collect());
    // `GameResult::final_event_award_divergences`'s own computation: sum
    // `final_event_awards_posthoc`'s per-card STEPS (a ranked card carries
    // two entries per seat -- `scoring_culture`'s own award, then a
    // `rankingCulture` bonus -- the journal's line states their COMBINED
    // total, so these must be summed before comparing) and diff against
    // [`parse_final_event_journal_amounts`]'s ground truth.
    let final_event_award_divergences = match &final_event_awards_posthoc {
        None => Vec::new(),
        Some(awards) => {
            let journal_amounts = parse_final_event_journal_amounts(journal, card_index);
            let mut out = Vec::new();
            for (card, steps) in awards {
                let Some(journal_seats) = journal_amounts.get(card) else { continue };
                for &(seat, journal_amount) in journal_seats {
                    let engine_amount: i32 =
                        steps.iter().filter(|&&(idx, _)| idx == seat).map(|&(_, amt)| amt).sum();
                    if engine_amount != journal_amount {
                        out.push(FinalEventAwardDivergence { card: card.name(), seat, journal_amount, engine_amount });
                    }
                }
            }
            out
        }
    };

    let ever_cards_in_play = game_ever_cards_in_play(&r.state, &r.moves_applied, &r.events_resolved);
    GameResult {
        id: meta.id.clone(),
        players: meta.players,
        actions_consumed: r.actions_consumed,
        trailing_discards: r.trailing_discards_lines.len(),
        duplicate_builds: r.duplicate_build_lines.len(),
        completed: completed && mismatch.is_none(),
        mismatch,
        colonize_approximated: r.colonize_approximated,
        bid_ceilings_grounded: r.bid_ceilings_grounded,
        hand_full_takes_overridden: r.hand_full_takes_overridden,
        engine_scores,
        index_scores: meta.scores.clone(),
        final_event_cards,
        final_event_award_divergences,
        final_event_awards_computed: final_event_awards_posthoc
            .map(|awards| {
                awards
                    .into_iter()
                    .map(|(card, steps)| (card.name(), steps))
                    .collect()
            })
            .unwrap_or_default(),
        discards_solved: r.discard_solver.solved,
        discards_chosen: r.discard_solver.chosen,
        discards_forced_collision: r.discard_solver.forced_collisions,
        decisions: r.decisions,
        discard_oracle_divergence: r.discard_oracle_divergence,
        discard_oracle_checked: r.discard_oracle_checked,
        discard_oracle_agreed: r.discard_oracle_agreed,
        hand_ledger_verdict: r.hand_ledger_verdict,
        culture_oracle_divergence: r.culture_oracle_divergence,
        culture_oracle_checked: r.culture_oracle_checked,
        culture_oracle_agreed: r.culture_oracle_agreed,
        science_oracle_divergence: r.science_oracle_divergence,
        science_oracle_checked: r.science_oracle_checked,
        science_oracle_agreed: r.science_oracle_agreed,
        resource_oracle_divergence: r.resource_oracle_divergence,
        resource_oracle_checked: r.resource_oracle_checked,
        resource_oracle_agreed: r.resource_oracle_agreed,
        civil_deck_premature_advance,
        politics_false_skips: r.politics_false_skips,
        politics_false_skips_unrecovered: r.politics_false_skips_unrecovered,
        cards_in_play: game_cards_in_play(&r.state),
        corruption_checks: r.corruption_checks,
        ever_cards_in_play,
        moves_applied: r.moves_applied,
        line_classes: r.line_classes,
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
                    if crate::debugflags::replay_debug() {
                        let p = &r.state.players[actor as usize];
                        eprintln!(
                            "DEBUG free_civil_action_move gap: wanted={wanted:?} landed_in_techs={landed_in_techs:?} science={} hand_civil={:?} tech_cost_net(landed)={:?}",
                            p.science,
                            p.hand_civil.as_slice().iter().map(|id| id.get().name).collect::<Vec<_>>(),
                            costs::tech_cost_net(&r.state, p, landed_in_techs),
                        );
                    }
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

/// Whether a rejected `"<Color> bids N"` line is rejected SPECIFICALLY
/// because this binary's own colonization ceiling for that bidder is too
/// low, returning that ceiling. `None` (keep the original `IllegalMove`)
/// for every other shape: a bid that is not a genuine raise over the
/// standing one, a bid by somebody who is not the auction's current
/// decider, an auction that has already closed by the time this line is
/// reached. Those are different problems and deserve their own honest
/// report rather than being folded in here.
///
/// The ceiling can be too low because `interact::max_force` reads
/// `p.hand_military` directly, and a military BONUS card enters a real
/// player's hand via an anonymous end-of-turn draw: unless the journal
/// later shows it played, this binary is holding SIMULATED filler in that
/// slot (see the module doc's "RECONSTRUCTED vs SIMULATED"). §11.3's
/// colonization force is therefore only a LOWER bound for any bidder who
/// might be holding one -- which [`Replayer::ground_bid_ceiling`] then
/// raises, from the bid itself.
fn bid_exceeds_ceiling(r: &Replayer, actor: u8, n: u8) -> Option<i32> {
    let Some(Pending::Auction(a)) = r.state.pending.top() else { return None };
    if a.player != actor || n as i32 <= a.bid as i32 {
        return None;
    }
    let ceiling = crate::interact::max_force(&r.state, &r.state.players[a.player as usize]);
    (n as i32 > ceiling).then_some(ceiling)
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
    // `cardblame`'s ActionClass-keyed happenings hook (task 2026-08-14):
    // every journal line this file classifies, in order, whether or not
    // its own dispatch arm below ends up applying a `Move` at all -- see
    // `Replayer::line_classes`'s own doc for why several `ActionClass`
    // variants (pure confirmation lines) have no `Move` of their own and so
    // are ONLY visible via this push.
    r.line_classes.push(class);
    if crate::debugflags::replay_debug_all() {
        let p = &r.state.players[actor as usize];
        eprintln!(
            "DEBUG APPLY_ONE ENTRY: actor={actor} class={class:?} card={:?} raw_text={raw_text:?} hand_civil_before={:?}",
            card.map(|c| c.get().name),
            p.hand_civil.as_slice().iter().map(|id| id.get().name).collect::<Vec<_>>(),
        );
    }
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
            // International Agreement's `Pending::Choice(TakeRow)` left open
            // for `actor` (`resolve_intervening`'s `ChoiceKind::TakeRow`
            // handling stopped here on purpose, deferring to this arm, which
            // has the parsed card -- see its own doc). BGO logs each pick
            // the same `"takes ... in hand ... uses N civil action"` way an
            // ordinary `Move::Take` uses, but a bare `Take` is illegal while
            // this pending sits open (`legal::legal_moves`'s pending gate
            // offers only `Choose`) -- translate into the option naming this
            // same slot instead.
            if let Some(Pending::Choice(c)) = r.state.pending.top() {
                if matches!(c.kind, ChoiceKind::TakeRow { .. }) {
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Slot(s) if *s == slot))
                        .ok_or_else(|| {
                            MismatchKind::ParserGap(format!(
                                "open TakeRow choice does not offer slot {slot} ({})",
                                card.get().name
                            ))
                        })?;
                    r.try_apply(Move::Choose { n: n as u8 }, true)?;
                    // Same refill-ungrounding as the ordinary `Move::Take`
                    // path just below -- see that comment for why.
                    r.row_grounded[slot as usize] = false;
                    return Ok(());
                }
            }
            // `IllegalMove: Take` diagnostic (docs/REPLAY.md's Take/Bid
            // handoff): a slot was found whose `take_cost` reproduces the
            // journal's own stated cost, but `try_apply` may still reject it
            // -- name WHICH `costs::take_rejection` gate fires, using the
            // engine's own gate/cost functions directly (not a
            // reimplementation), rather than leaving only the generic
            // "illegal move" dump `try_apply` already prints.
            if crate::debugflags::replay_debug() {
                let p = &r.state.players[actor as usize];
                let gate = costs::take_gate(&r.state, p, None);
                if let Some(reason) = costs::take_rejection(&r.state, p, slot as usize, &gate) {
                    let s = effects::state_stats(&r.state, p);
                    eprintln!(
                        "DEBUG TAKE REJECT: lineno={} age_civil={:?} card={} slot={slot} reason={reason:?} our_take_cost={} \
                         journal_cost={cost} gate_have={} civil_actions={} military_actions={} \
                         leader={} government={} hand_civil_size={} civil_hand_limit={} s_civil_actions={} s_civil_hand_limit={} \
                         completed_wonders={:?} hand_civil={:?} raw_text={raw_text:?}",
                        r.current_lineno,
                        r.state.age_civil,
                        card.get().name,
                        costs::take_cost(&r.state, p, slot as usize),
                        gate.have,
                        p.civil_actions,
                        p.military_actions,
                        if p.leader.is_none() { "none" } else { p.leader.get().name },
                        p.government.get().name,
                        p.hand_size_civil(),
                        costs::civil_hand_limit(&r.state, p),
                        s.civil_actions,
                        s.civil_hand_limit,
                        p.completed_wonders.as_slice().iter().map(|id| id.get().name).collect::<Vec<_>>(),
                        p.hand_civil.as_slice().iter().map(|id| id.get().name).collect::<Vec<_>>(),
                    );
                }
            }
            r.try_apply_take(actor, slot)?;
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
            maybe_synthesize_churchill_military(r, actor, raw_text)?;
            if let (Some(want), Some(got)) = (
                stated_build_payment(raw_text),
                costs::build_cost_for(&r.state, &r.state.players[actor as usize], card),
            ) {
                if want != got
                    && !build_discount_reconciles(&r.state, &r.state.players[actor as usize], card, want, got)
                {
                    return Err(MismatchKind::UnrecoverableHiddenInfo(format!(
                        "build cost mismatch for {}: journal says {want} resources, this binary's \
                         reconstructed state computes {got} -- an unmodeled discount source (not a \
                         parser gap: the card and actor are both correctly resolved)",
                        card.get().name
                    )));
                }
                convert_trade_food_shortfall(r, actor, raw_text, got)?;
            }
            r.try_apply(Move::Build { card }, true)
        }
        ActionClass::BuildWonderStage => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("wonder stage with no resolved card".into()))?;
            let after_builds = rest.strip_prefix("builds ").ok_or_else(|| MismatchKind::ParserGap("wonder-stage line missing 'builds '".into()))?;
            let steps = wonder_stage_count(after_builds).ok_or_else(|| MismatchKind::ParserGap("could not parse wonder stage count".into()))?;
            let _ = card; // the wonder itself is implicit in state (under construction)
            // Same Trade Routes fold as the ordinary Build arm just above
            // (`convert_trade_food_shortfall`'s own doc) -- a wonder stage is
            // priced in resources exactly like any other build, and BGO folds
            // the conversion into this line the identical way (e.g. game
            // `7523070` line 166, `"Green builds 1 stage of Taj Mahal Green
            // spends 1 resource; Green spends 1 food"`). No `want != got`
            // pre-check here (unlike Build): `convert_trade_food_shortfall`'s
            // own internal gate already refuses to act unless the journal's
            // stated total exactly matches this binary's own cost, so an
            // unrelated wonder-cost bug still surfaces as the ordinary
            // `IllegalMove` it always has, not a manufactured pass.
            //
            // Gated on `p.wonder` actually being set FIRST:
            // `costs::wonder_stage_cost` panics otherwise (a `debug_assert`,
            // and this binary's `difftest` profile inherits `panic = "abort"`
            // from `[profile.release]`, so a stray panic here would abort the
            // WHOLE corpus run rather than just this one game/line -- see
            // this crate's own standing warning on that). A reconstruction
            // whose state has already diverged enough that it thinks no
            // wonder is under construction here was never going to complete
            // this line correctly anyway; skipping the conversion and falling
            // through to the ordinary `try_apply` reproduces its previous,
            // honest failure instead of a hard abort.
            if !r.state.players[actor as usize].wonder.is_none() {
                let true_cost = costs::wonder_stage_cost(&r.state, &r.state.players[actor as usize], steps);
                convert_trade_food_shortfall(r, actor, raw_text, true_cost)?;
            }
            // Same BGO redundant double-log the ordinary Build arm skips,
            // but for a wonder stage: BGO logs a single `"<Color> builds
            // <N> stage(s) of <Wonder>"` action as two identical lines
            // seconds apart (e.g. `7522432`'s two "builds 1 stage of Fast
            // Food Chains" lines; `7522632`'s two "builds 1 stage of Fast
            // Food Chains" before the "Wonder completed" trailer). Re-applying
            // the second `Move::WonderStep{steps}` over-spends the stage's
            // resources, starving a later genuinely-unaffordable build into
            // `IllegalMove`. Skip the redundant line when a `Move::WonderStep
            // {steps}` for THIS actor was ALREADY applied this turn AND
            // re-applying it is now ILLEGAL in the reconstructed state
            // (`Move::WonderStep{steps}` absent from `legal_moves`) -- the
            // same two-condition guard the Build arm uses. A GENUINE second
            // stage of the same wonder (e.g. building a 4-stage wonder's
            // stages across two separate actions, or a `steps` value still
            // legal because the wonder is far from complete) applies legally
            // and never satisfies the illegality condition, so it is never
            // skipped. A `FreeBuild`/action-card wonder build resolves
            // through `Move::Choose`, never a bare `Move::WonderStep`, so it
            // never reaches this branch either.
            {
                let mv = Move::WonderStep { steps };
                let actor_applied_wonder = r.state.decider() == actor;
                let already_applied = r
                    .moves_applied
                    .iter()
                    .any(|m| matches!(m, Move::WonderStep { steps: s } if *s == steps));
                let now_illegal = !legal::legal_moves(&r.state).as_slice().contains(&mv);
                if actor_applied_wonder && already_applied && now_illegal {
                    r.duplicate_build_lines.push(r.current_lineno);
                    return Ok(());
                }
            }
            r.try_apply(Move::WonderStep { steps }, true)
        }
        ActionClass::IncreasePopulation => {
            let legal = legal::legal_moves(&r.state);
            // BGO's own line names the free-population source explicitly
            // when one was used ("Ocean Liner Service used" -- the wonder's
            // once-per-turn free Increase Population, §9, mirrored by
            // `Move::PopFree`/`h_pop_free`). ENGINE BUG (found chasing the
            // `IllegalMove: Take` bucket, game `7523341` round 14): this
            // arm used to try `Move::Pop` FIRST whenever it happened to be
            // legal, ignoring that signal entirely -- a player with both a
            // spare civil action AND the free Ocean Liner option still
            // available took the free one in reality (the journal's own
            // "Ocean Liner Service used" says so), but this binary silently
            // spent a civil action instead, permanently overcharging
            // `p.civil_actions` for the rest of that turn until it starved
            // a later `Take`. The SAME line's text is unambiguous here --
            // trust it exactly the way this file already trusts every
            // other named-source journal clause -- and only fall back to
            // the "whichever is legal" guess when the text does not say.
            let free_source_named = raw_text.contains("Ocean Liner Service used");
            // Trade Routes Agreement, side B ("Civilization B can use 1
            // resource as 1 food during its turn", §5.9): a player holding
            // the live grant may pay PART of a Pop cost in converted
            // resources -- BGO logs that as a SECOND clause on the SAME
            // line, not folded into the food number (`"<Color> increases
            // population <Color> spends N food; <Color> spends M resource"`).
            //
            // ENGINE BUG (the `IllegalMove: Pop` bucket, 6 games, e.g.
            // `7522432` round 7): this arm previously applied a plain
            // `Move::Pop` whenever one was legal, and only tried the
            // `Move::TradeResourceAsFood` conversion in the FALLBACK below
            // (i.e. only when Pop was already NOT legal). A player whose
            // plain food ALREADY covered the pop cost took the fast path,
            // the "spends M resource" clause was dropped, and this binary
            // overcharged food by M -- the conversion this player actually
            // used in the true game never happened. The drift surfaced a
            // few actions later as an unaffordable Pop/Build/Develop, far
            // from its cause (the same class of "the conversion is in the
            // wrong branch" bug the Build arm's `convert_trade_food_shortfall`
            // closes on the other direction). Apply the conversion on the
            // fast path too, gated EXACTLY as the fallback: only when the
            // journal's OWN stated total (food clause + the resource
            // clause) matches this binary's `pop_cost`, the Trade-Resources-
            // as-Food grant covers it, and the player has the resources.
            // Without the gate an unrelated food/resource drift still
            // surfaces as the ordinary `IllegalMove` it always has -- we
            // never loosen a check to hide a mismatch (docs/REPLAY.md's
            // Civil Life warning).
            if free_source_named {
                r.try_apply(Move::PopFree, true)
            } else {
                let p = &r.state.players[actor as usize];
                let stated = spent_food(raw_text).map(|f| f + spent_resource_after_food(raw_text));
                let cost = crate::economy::pop_cost(&r.state, p);
                let resource_clause = spent_resource_after_food(raw_text);
                let can_convert = matches!(
                    (stated, cost),
                    (Some(s), Some(c)) if s == c
                ) && resource_clause > 0
                    && resource_clause <= crate::economy::trade_resource_as_food_remaining(&r.state, p)
                    && resource_clause <= p.resources as i32;
                if can_convert {
                    for _ in 0..resource_clause {
                        r.try_apply(Move::TradeResourceAsFood, true)?;
                    }
                    let legal = legal::legal_moves(&r.state);
                    if legal.as_slice().contains(&Move::Pop) {
                        r.try_apply(Move::Pop, true)
                    } else if legal.as_slice().contains(&Move::PopFree) {
                        r.try_apply(Move::PopFree, true)
                    } else {
                        Err(MismatchKind::IllegalMove {
                            attempted: "Pop after TradeResourceAsFood".into(),
                            legal_moves: format!("{:?}", legal.as_slice()),
                        })
                    }
                } else if legal.as_slice().contains(&Move::Pop) {
                    r.try_apply(Move::Pop, true)
                } else if legal.as_slice().contains(&Move::PopFree) {
                r.try_apply(Move::PopFree, true)
            } else { let res = {
                // Trade Routes Agreement, side B ("Civilization B can use 1
                // resource as 1 food during its turn", §5.9): a player
                // holding the live grant may pay PART of a Pop cost in
                // converted resources -- BGO logs that as a SECOND clause on
                // the SAME line, not folded into the food number (an
                // earlier version of this comment assumed otherwise and was
                // wrong -- see `spent_food`'s own doc comment, found chasing
                // the `IllegalMove: Pop` bucket, `docs/REPLAY.md`): `"<Color>
                // increases population <Color> spends N food; <Color> spends
                // M resource"`. This binary's `Move::TradeResourceAsFood`
                // already exists (docs/REPLAY.md's Trade Routes Agreement
                // engine fix) but this handler never tried it, one-sidedly
                // leaving every partly-resource-paid Pop illegal. Gated on
                // the journal's OWN stated TOTAL (food clause plus the
                // second resource clause, if any) matching this binary's
                // `pop_cost` exactly -- if they disagree, the fault is a
                // mispriced pop_cost (yellow-bank drift, a missing discount,
                // ...), not a missing conversion, and spending resources
                // here would only mask that bug behind a
                // wrong-for-a-different-reason success (`docs/REPLAY.md`'s
                // Civil Life warning: never loosen a check just to make a
                // mismatch disappear).
                let p = &r.state.players[actor as usize];
                let stated = spent_food(raw_text).map(|f| f + spent_resource_after_food(raw_text));
                let cost = crate::economy::pop_cost(&r.state, p);
                let shortfall = match (stated, cost) {
                    (Some(stated), Some(cost)) if stated == cost => cost - p.food as i32,
                    _ => 0,
                };
                shortfall > 0
                    && shortfall <= crate::economy::trade_resource_as_food_remaining(&r.state, p)
                    && shortfall <= p.resources as i32
            }; if res {
                let p = &r.state.players[actor as usize];
                let cost = crate::economy::pop_cost(&r.state, p).expect("checked above");
                let shortfall = cost - p.food as i32;
                for _ in 0..shortfall {
                    r.try_apply(Move::TradeResourceAsFood, true)?;
                }
                let legal = legal::legal_moves(&r.state);
                if legal.as_slice().contains(&Move::Pop) {
                    r.try_apply(Move::Pop, true)
                } else {
                    // The conversion(s) landed but Pop is still not legal
                    // (e.g. no civil action left) -- the same honest
                    // failure as before, just past a fixed shortfall.
                    Err(MismatchKind::IllegalMove {
                        attempted: "Pop after TradeResourceAsFood".into(),
                        legal_moves: format!("{:?}", legal.as_slice()),
                    })
                }
            } else {
                // Neither is legal -- almost always food/yellow-bank drift
                // from an earlier build/economy step this binary priced
                // differently than the true game (see the module doc's
                // "gives up on" list and `docs/REPLAY.md`'s mismatch
                // categories), not a parser gap in THIS line.
                if crate::debugflags::replay_debug() {
                    let p = &r.state.players[actor as usize];
                    eprintln!(
                        "DEBUG Pop fail: food={} yellow_bank={} civil_actions={} pop_cost={:?} round={} numplayers={} lineno={} otd_pop_food={} leader={} government={} pending={:?} raw={:?}",
                        p.food, p.yellow_bank, p.civil_actions,
                        crate::economy::pop_cost(&r.state, p), r.state.round, r.state.num_players,
                        r.current_lineno, p.one_time_discount.pop_food,
                        if p.leader.is_none() { "none" } else { p.leader.get().name },
                        p.government.get().name,
                        r.state.pending.top(), raw_text
                    );
                }
                Err(MismatchKind::IllegalMove {
                    attempted: "Pop or PopFree".into(),
                    legal_moves: format!("{:?}", legal.as_slice()),
                })
            } }
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
            maybe_synthesize_churchill_military(r, actor, raw_text)?;
            // Same Trade Routes fold as `ActionClass::BuildBuilding`'s Build
            // arm (`convert_trade_food_shortfall`'s own doc) -- an upgrade is
            // priced in resources exactly like a build, and BGO folds the
            // conversion into this line the identical way.
            let true_cost = costs::upgrade_cost(&r.state, &r.state.players[actor as usize], from, to);
            convert_trade_food_shortfall(r, actor, raw_text, true_cost)?;
            r.try_apply(Move::Upgrade { from, to }, true)
        }
        // J. S. Bach's leader ability -- same "<From> to <To>" shape as an
        // ordinary Upgrade (including the same "using <Card>" ordered-free-
        // action wrapper, e.g. `"Bachupgrades Religion to Opera using
        // Efficient Upgrade"`), but the MOVE is `Move::BachTheater`, not
        // `Move::Upgrade`: Bach is what makes a cross-family Temple/Library
        // -> Theater conversion legal at all (an ordinary `Move::Upgrade`
        // only ever offers same-family targets, `legal.rs`'s own `higher`
        // filter), and it separately marks `bach_upgrade_used` (once per
        // turn) `apply::h_bach_theater` -- see `ActionClass::BachTheater`'s
        // own doc comment for why this used to be dropped entirely.
        ActionClass::BachTheater => {
            let to = card.ok_or_else(|| MismatchKind::ParserGap("Bach upgrade with no resolved target card".into()))?;
            let after_upgrades = rest.strip_prefix("upgrades ").ok_or_else(|| MismatchKind::ParserGap("Bach upgrade line missing 'upgrades '".into()))?;
            let from = upgrade_from_card(r.card_index, after_upgrades)
                .ok_or_else(|| MismatchKind::ParserGap("could not parse Bach upgrade source card".into()))?;
            if free_civil_action_move(r, actor, rest, Move::BachTheater { from, to }, to)? {
                return Ok(());
            }
            r.try_apply(Move::BachTheater { from, to }, true)
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
            maybe_synthesize_churchill_military(r, actor, raw_text)?;
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
                    crate::cards::Special::A(_) | crate::cards::Special::AllPlayers(_) | crate::cards::Special::B(_) | crate::cards::Special::BestTheaterDoubleCulture | crate::cards::Special::BothPlayers(_) | crate::cards::Special::BuildDiscount(_) | crate::cards::Special::CancelledIfPartiesAttackEachOther | crate::cards::Special::CannotPlayAggressionOrWar | crate::cards::Special::CivilActionBackOnTechDevelop(_) | crate::cards::Special::CivilActionUpgradeUrbanBuildingToTheater | crate::cards::Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | crate::cards::Special::ColonyImmediateBonusApplies | crate::cards::Special::ColonyPermanentBonusTransfers | crate::cards::Special::ComboFoodDiscount(_) | crate::cards::Special::ComboResourceDiscount(_) | crate::cards::Special::Condition(_) | crate::cards::Special::CultureFirstColony(_) | crate::cards::Special::CultureIfTopTwoStrength(_) | crate::cards::Special::CultureOnLeaveEqualToLabResourceProduction | crate::cards::Special::CultureOnRevolution(_) | crate::cards::Special::CultureOnTechDevelop(_) | crate::cards::Special::CulturePerAdditionalColony(_) | crate::cards::Special::CulturePerCivilizationWithMoreCulture(_) | crate::cards::Special::CulturePerHappyFromTemplesTheatersWonders(_) | crate::cards::Special::CulturePerLabEqualToLevel | crate::cards::Special::CulturePerLibraryTheaterPair(_) | crate::cards::Special::CulturePerTheater(_) | crate::cards::Special::DecreasePopulation(_) | crate::cards::Special::DestroyUrbanBuildings(_) | crate::cards::Special::DoubleBestMine | crate::cards::Special::DoublesTacticBonusOfOneArmy | crate::cards::Special::ExtraHappyPerHappySource(_) | crate::cards::Special::FinalScoring(_) | crate::cards::Special::FreePopIncreasePerTurn | crate::cards::Special::Gain(_) | crate::cards::Special::GainCulturePerLevelOfRemovedCard(_) | crate::cards::Special::GainFoodOrResources(_) | crate::cards::Special::GainResources(_) | crate::cards::Special::InfantryCountsAsCavalryForTactics | crate::cards::Special::LastRoundSubstitute(_) | crate::cards::Special::LeaderTakeCivilActionDiscount(_) | crate::cards::Special::LibraryDiscountsIfTheater | crate::cards::Special::Lose(_) | crate::cards::Special::MilitaryActionAsCivilPerTurn(_) | crate::cards::Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | crate::cards::Special::NoAttacksBetweenParties | crate::cards::Special::OnAttackBetweenParties(_) | crate::cards::Special::OnBuildCulture(_) | crate::cards::Special::OnBuildCulturePerTechLevelSum | crate::cards::Special::OnReplacePutUnderCompletedWonderHappy(_) | crate::cards::Special::OncePerGameTwoPoliticalActions | crate::cards::Special::OpponentDecreasesPopulation(_) | crate::cards::Special::OpponentsPayDoubleMilitaryActionsToAttackYou | crate::cards::Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | crate::cards::Special::PeekTopEventCardInPolitics | crate::cards::Special::PerTurnChoice | crate::cards::Special::PlayerWithLeastCulture(_) | crate::cards::Special::PlayerWithMostCulture(_) | crate::cards::Special::PlayersWithMostDiscontentWorkers(_) | crate::cards::Special::PlayersWithMostHappyFaces(_) | crate::cards::Special::PopIncreaseFoodDiscount(_) | crate::cards::Special::RemoveAsPoliticalActionForYellowToken(_) | crate::cards::Special::RemoveAsPoliticalActionFreeColonize | crate::cards::Special::RemoveFromGame | crate::cards::Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | crate::cards::Special::ResourceOnTechDevelop(_) | crate::cards::Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | crate::cards::Special::ResourcesPerLabEqualToLevel | crate::cards::Special::RevolutionUsesMilitaryActionsInstead | crate::cards::Special::ScienceOnTechCardTake(_) | crate::cards::Special::SciencePerBestLabOrLibraryLevel | crate::cards::Special::SciencePerLab(_) | crate::cards::Special::StealColony(_) | crate::cards::Special::StrengthPerArtillery(_) | crate::cards::Special::StrengthPerInfantry(_) | crate::cards::Special::StrengthPerMilitaryUnit(_) | crate::cards::Special::StrengthPerTempleOrGovernmentHappy(_) | crate::cards::Special::StrengthPerUnitType(_) | crate::cards::Special::StrongestPlayer(_) | crate::cards::Special::StrongestPlayers(_) | crate::cards::Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | crate::cards::Special::TakeFromOpponent(_) | crate::cards::Special::TheaterResourceDiscountIfLibrary(_) | crate::cards::Special::TheaterScienceDiscountIfLibrary(_) | crate::cards::Special::TheaterTechScienceDiscount(_) | crate::cards::Special::VictorTakesCulture | crate::cards::Special::VictorTakesScienceUpTo(_) | crate::cards::Special::VictorTakesYellowTokens | crate::cards::Special::WeakestPlayer(_) | crate::cards::Special::WeakestPlayers(_) | crate::cards::Special::WonderTakeNoExtraCivilActions => None,
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
            }
            // Patriotism (`Special::FreeCivilAction` is not set on it at
            // all, so the `kind` match above never even tries) prints its
            // own age-dependent `resourcesForMilitaryUnits` bonus directly
            // on this line -- see `trailing_gets_military_resource`'s own
            // doc comment for why this must be checked independently of
            // `kind` rather than folded into that match.
            .or_else(|| {
                trailing_gets_military_resource(raw_text)
                    .and_then(|n| family_siblings(named).into_iter().find(|id| id.get().effects.resources_for_military_units as i32 == n))
            })
            // Reserves (`Special::GainFoodOrResources`, no `FreeCivilAction`
            // either) prints its own age-dependent magnitude (2/3/4 for
            // Age I/II/III) right there in the SAME trailing "produces N
            // food/resources" clause `ChoiceKind::FoodOrRes` resolution
            // below already parses for the food-vs-resources KIND -- the
            // magnitude `n` it also returns pins the CARD identity down the
            // same way, and was sitting unused before this fix: whichever
            // age-sibling the earlier `"takes Reserves in hand"` line
            // guessed (age-blind, `best_age_sibling`'s doc comment) got
            // played and its OWN (possibly wrong) gain applied instead.
            .or_else(|| {
                trailing_produces(raw_text).and_then(|(_, n)| {
                    family_siblings(named).into_iter().find(|id| {
                        id.get()
                            .special
                            .iter()
                            .any(|s| matches!(s, crate::cards::Special::GainFoodOrResources(v) if *v as i32 == n))
                    })
                })
            })
            // Cultural Heritage (`gainScience` 1/2 for age A/I) and
            // Revolutionary Idea (`gainScience` 4/6 for age II/III): neither
            // has a `FreeCivilAction`/other `Special` at all (`special:
            // &[]`), and neither prints "military resource" or a
            // food/resources "produces" clause, so none of the three
            // clauses above ever fire for them -- the SAME "no kind match,
            // `solved` stays `None`, trust whatever `best_age_sibling`
            // guessed at take time" gap the Patriotism/Reserves fixes
            // closed for their own families (`docs/REPLAY.md`'s
            // Build/Upgrade/WonderStep handoff named these two as
            // unchecked). Reuses `trailing_gets_science` (already used for
            // Breakthrough's `Move::Develop`/`Move::Revolution` case) --
            // corpus-confirmed it does not confuse Cultural Heritage's
            // trailing "<Color> scores 4 culture" clause with the "<Color>
            // gets N science" one before it (`rfind(" gets ")` only ever
            // sees the "gets" clause, "scores" has no "gets" substring).
            // Gated on `base_name`, unlike Patriotism/Reserves above, which
            // gate implicitly through their own `Special`/text shape --
            // there is no such self-gating signal for a bare `gainScience`
            // number, so an ungated version could misfire on an unrelated
            // card that happens to print a coincidentally-matching "gets N
            // science" clause.
            .or_else(|| {
                matches!(named.get().base_name, "Cultural Heritage" | "Revolutionary Idea")
                    .then(|| trailing_gets_science(raw_text))
                    .flatten()
                    .and_then(|n| family_siblings(named).into_iter().find(|id| id.get().effects.gain_science as i32 == n))
            });
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
            if kind == Some(legal::FreeActionKind::IncreasePopulation) {
                // Trade Routes Agreement, side B (§5.9, "Civilization B can
                // use 1 resource as 1 food during its turn"): the SAME
                // shortfall-covering conversion the ordinary
                // `ActionClass::IncreasePopulation` arm above already makes
                // (that arm's own doc comment has the full citation) --
                // Frugality's ordered Pop resolves at full price (this
                // file's own RESOLVED note: gains land AFTER the ordered
                // action), so a player short on food may still cover PART
                // of it with a live Trade Routes conversion, and BGO logs
                // that as a second `"<Color> spends M resource"` clause
                // glued onto THIS "plays Frugality" line rather than a
                // separate line the way `Move::TradeResourceAsFood` itself
                // would print. Found chasing the `IllegalMove: PlayAction`
                // bucket (game 7522830, round 8): this arm never tried it,
                // leaving every partly-resource-paid Frugality illegal.
                let res = {
                    let p = &r.state.players[actor as usize];
                    let stated = spent_food(raw_text).map(|f| f + spent_resource_after_food(raw_text));
                    let cost = crate::economy::pop_cost(&r.state, p);
                    let shortfall = match (stated, cost) {
                        (Some(stated), Some(cost)) if stated == cost => cost - p.food as i32,
                        _ => 0,
                    };
                    shortfall > 0
                        && shortfall <= crate::economy::trade_resource_as_food_remaining(&r.state, p)
                        && shortfall <= p.resources as i32
                };
                if res {
                    let p = &r.state.players[actor as usize];
                    let cost = crate::economy::pop_cost(&r.state, p).expect("checked above");
                    let shortfall = cost - p.food as i32;
                    for _ in 0..shortfall {
                        r.try_apply(Move::TradeResourceAsFood, true)?;
                    }
                }
            }
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
                    let (is_resources, _n) = trailing_reserves_gain(raw_text).ok_or_else(|| {
                        MismatchKind::ParserGap(format!(
                            "PlayAction {{{}}} opened a FoodOrRes choice but no trailing \
                             \"produces\"/\"spends\" clause found in {raw_text:?}",
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
                // `ChoiceKind::LosePop` (a forced "lose 1 population" with no
                // free worker to absorb it -- `interact::run_item`'s
                // `QueueItem::LosePop` arm) is the SAME shape as `DestroyOwn`
                // here: BGO logs the player's pick the same "<Color> destroys
                // <Card>" way, and the pending's own options are the same
                // `ChoiceOption::Card` list either kind produces
                // (`worker_holding_options`). Previously only `DestroyOwn`
                // was recognised, so a real LosePop resolution fell through
                // to a bare `Move::Destroy` -- illegal while ANY pending sits
                // open (`legal::legal_moves`'s own top-level pending gate) --
                // reported as `IllegalMove: Destroy`. Found chasing the
                // `IllegalMove: Pop` bucket (`docs/REPLAY.md`): a stray
                // `LosePop` from an earlier event/aggression leaves this gap
                // reachable on the SAME player's very next real "destroys"
                // line, at whatever later point in the journal it appears.
                if matches!(c.kind, ChoiceKind::DestroyOwn | ChoiceKind::LosePop) {
                    let n = c
                        .options
                        .as_slice()
                        .iter()
                        .position(|o| matches!(o, ChoiceOption::Card(id) if *id == card))
                        .ok_or_else(|| MismatchKind::ParserGap(format!("observed destroy card not among {:?} options", c.kind)))?;
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
                r.consume_named_military_card(actor, card, Move::PlayTactic { card }, true)
            }
        }
        ActionClass::DeclareWar => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("war with no resolved card".into()))?;
            let target = color_after(rest, " on ").ok_or_else(|| MismatchKind::ParserGap("could not parse war target colour".into()))?;
            r.consume_named_military_card(actor, card, Move::War { card, target: target.seat() }, true)
        }
        ActionClass::WinWar => Ok(()), // automatic (game::resolve_war_outcome); validation checkpoint only
        ActionClass::PlayAggression => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("aggression with no resolved card".into()))?;
            // `corpus::build_card_index`'s bare-name lookup for "Plunder" is
            // an arbitrary same-name pick (`best_age_sibling`'s own doc
            // comment) -- Plunder prints a different "up to N resource
            // and/or food" per age (3/5/7), and `combat::finish_aggression`
            // opens the `PlunderSplit` choice capped at THAT printed number,
            // so resolving the wrong age caps the choice too low (or, just
            // as easily, too high -- see below) to ever match the journal's
            // own resolution line (analysis/worker_notes_2026-08-14/
            // plunder__PLUNDER.txt, game 7523357).
            //
            // `best_age_sibling(card, r.state.age_military)`'s "highest age
            // not exceeding the current one" bound was tried first and
            // rejected: confirmed on game `7521906` (line 135, "up to 3" --
            // Age I) that a card DRAWN at an earlier age and never played
            // survives an age transition unplayed, so `age_military` can
            // already be II by the time it is finally played, and the bound
            // then picks the WRONG-NEWER Age II sibling -- the exact
            // opposite failure from the original bug, caused by the same
            // guess. The journal's own "up to N" clause pins the age down
            // exactly, with no guessing in either direction, so read that
            // instead.
            //
            // Scoped to Plunder only, not every Aggression card (Raid,
            // Enslave, Spy, Annex, Infiltrate, ...) -- those share the same
            // age-blind shape in principle but were not verified against
            // their own card data this session; see the pickup plan in
            // `analysis/worker_notes_2026-08-14/discardsolve__
            // DISCARDSOLVE.txt`.
            let card = resolve_plunder_age(card, raw_text);
            let card = resolve_aggression_age(card, raw_text);
            let target = color_after(rest, " against ").ok_or_else(|| MismatchKind::ParserGap("could not parse aggression target colour".into()))?;
            r.consume_named_military_card(actor, card, Move::Aggression { card, target: target.seat() }, true)?;
            resolve_aggression_defense(r, next_text)
        }
        ActionClass::ProposePact => {
            let card = card.ok_or_else(|| MismatchKind::ParserGap("pact with no resolved card".into()))?;
            let target = color_after(rest, " to ").ok_or_else(|| MismatchKind::ParserGap("could not parse pact target colour".into()))?;
            let side = pact_side(raw_text, target_actor_color(actor), card);
            r.consume_named_military_card(actor, card, Move::OfferPact { card, target: target.seat(), side }, true)
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
            //
            // ENGINE/REPLAYER BUG (found chasing the `WinWar` culture-oracle
            // delta on game `7522152`): `interact::colonize`'s own
            // `colonize_auto` can walk a `Pending::Colonize` all the way to
            // completion by itself, with NO branching step, whenever every
            // one of its steps happens to have exactly one legal
            // continuation (a small unit pool with no spare bonus cards is
            // the common shape). That path runs synchronously, inside
            // whatever earlier `Bid`/`BidPass` line triggered the auction
            // win -- long before `resolve_intervening`'s own `Pending::
            // Colonize` check (which is what calls `drain_colonize`, the
            // ONLY other place that pops `colonize_sacrifices`) ever gets a
            // chance to see this pending open. `drain_colonize` is
            // therefore never called for a fully-forced colonize, its
            // `ColonizeSacrifice` entry is never popped, and it sits at the
            // front of the per-game queue forever -- silently misattributing
            // every LATER colonize in the same game (by any player) to the
            // wrong entry, which flunks `drain_colonize`'s own actor check
            // and falls back to `approximate_colonize`'s "sacrifice whatever
            // the engine offers first" heuristic instead of the journal's
            // real, exact choice. `approximate_colonize` can pick MORE or
            // DIFFERENT tableau units than the human actually sacrificed,
            // corrupting that player's own army strength for the rest of
            // the game -- the root cause of game `7522152`'s War over
            // Culture spoils computing an advantage of 24 instead of the
            // journal's 5 (Purple's own `army_strength` undercounted after
            // an earlier, wrongly-approximated colonize stripped units the
            // real Purple still had), a 19-culture swing on the very next
            // end-of-turn checkpoint.
            //
            // Fixed here, not in `interact.rs`: `colonize_sacrifices` and
            // `drain_colonize` are this file's own replay bookkeeping, not
            // engine state, so `interact::colonize_auto` completing a fully-
            // forced colonize is not itself wrong -- ENGINE state ends up
            // correct either way, `interact::colonize_moves` never offers an
            // illegal option.
            //
            // Only pop when the `Pending::Colonize` is ALREADY GONE: a
            // colonize that genuinely needs driving is, per this match arm's
            // own original comment and confirmed by tracing game
            // `7522152`'s SECOND (real, branching) colonize, still open
            // `Pending::Colonize` at the moment THIS confirmation line's own
            // `apply_one` dispatch runs -- the auction win that opened it is
            // an earlier line, but `resolve_intervening` (the only thing
            // that calls `drain_colonize`) does not run for a confirmation
            // line at all (`is_pure_confirmation_line`'s own doc), so this
            // dispatch is reached BEFORE the pending is drained, not after.
            // Popping unconditionally here (an earlier version of this fix)
            // stole the still-open entry out from under `drain_colonize`,
            // which then found the queue empty and fell back to
            // `approximate_colonize` for the exact case this fix exists to
            // avoid. Checking `state.pending.top()` first is what tells the
            // two cases apart: gone already means `colonize_auto` finished
            // it with no branching (this line is the only place left that
            // ever sees it, so pop now or leak it forever); still open means
            // a later `resolve_intervening` iteration owns popping it via
            // `drain_colonize`, same as before this fix existed.
            //
            // NOT ENOUGH ON ITS OWN, though (`worker FINALINPUT`, chasing
            // game `7522866`'s three-way final-award divergence on
            // Population/Competition/Variety, all seat0/Orange -- `analysis/
            // worker_notes_2026-08-14/finalinput__FINALINPUT.txt`):
            // `Pending::Colonize` reads "not open" in a SECOND, different
            // shape too -- the auction for THIS SAME territory has not been
            // settled YET, because `resolve_intervening`'s own auction-
            // forced-`BidPass` block (this file's "A live `Pending::Auction`"
            // comment, above) is what actually resolves a forced pass and
            // calls `interact::colonize`, and it only runs ahead of a
            // NON-confirmation line -- so for `is_pure_confirmation_line`
            // lines it has not run yet either, same reason `drain_colonize`
            // has not. That shape is indistinguishable from "already fully
            // auto-resolved" by `state.pending.top()` alone: both read as
            // "no `Pending::Colonize` right now". Traced on 7522866's own
            // "Orange colonizes a Developed Territory" line (II age): this
            // arm fired with `pending_open=false` and the queue's own front
            // already equal to THIS line's `Developed Territory` entry, so
            // the old check popped it -- but `interact::colonize` for this
            // exact territory had not run yet at that point (it ran moments
            // later, while `resolve_intervening` prepared the NEXT journal
            // line, via the forced-`BidPass` path). The popped entry was
            // real and still owed; `drain_colonize`, later, found the
            // FOLLOWING territory's entry at the front instead and force-fed
            // its clauses (`Unit(Knights)` first) against the wrong pending,
            // failed the legal-move check, and fell back to
            // `approximate_colonize` for BOTH this territory and the next
            // one -- silently sacrificing a different unit mix than Orange's
            // own journal-recorded choice for the rest of the game, one
            // fewer Swordsmen worker fielded than actually happened. That
            // permanently undercounts Orange's own worker/army-composition
            // state for every final-scoring formula that reads it
            // (`events.rs::scoring_culture`'s population/competition/variety
            // terms all read `p.techs` directly), which is why three
            // unrelated award FORMULAS all disagreed with the journal on the
            // same seat in the same game -- one shared upstream state bug,
            // not three formula bugs.
            //
            // The fix: gate the pop on a THIRD, purely structural signal
            // that cannot be true in the "auction not settled yet" shape --
            // `card` (this line's own territory) is already a completed
            // colony. `interact::gain_colony` (`colonize_step`'s `SendDone`
            // arm) is the ONLY place that pushes onto `p.colonies`, and it
            // runs at the very end of a `Pending::Colonize`'s resolution --
            // so `colonies.contains(card)` is true if and only if THIS
            // territory's own colonize, whether fully forced by
            // `colonize_auto` or manually driven by `drain_colonize`, has
            // already run to completion. In the "not settled yet" shape the
            // auction (and therefore the colonize) has not even started, so
            // this is always false there, correctly skipping the pop and
            // leaving the entry for `drain_colonize` to consume normally
            // once the real `Pending::Colonize` opens.
            let already_completed = card.is_some_and(|c| r.state.players[actor as usize].colonies.contains(c));
            if already_completed
                && !matches!(r.state.pending.top(), Some(Pending::Colonize(_)))
                && r.colonize_sacrifices.front().is_some_and(|s| s.lineno == r.current_lineno)
            {
                r.colonize_sacrifices.pop_front();
            }
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
                    let Some(ceiling) = bid_exceeds_ceiling(r, actor, n) else {
                        return Err(MismatchKind::IllegalMove { attempted, legal_moves });
                    };
                    if !r.ground_bid_ceiling(actor, n) {
                        return Err(MismatchKind::UnrecoverableHiddenInfo(format!(
                            "colonization bid of {n} exceeds this binary's computed force ceiling \
                             ({ceiling}) for the correctly-resolved bidder, and their whole \
                             military hand converted to the best bonus card the deck's current age \
                             prints still does not reach it -- so this is a genuine contradiction, \
                             not the ordinary hidden-hand gap"
                        )));
                    }
                    r.try_apply(Move::Bid { n }, true)
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
        // Unreachable for the same reason `RemoveLeaderYellow` above is:
        // `EndTurn` carries no leading actor colour, so `replay_game`'s own
        // dispatch loop special-cases it (actor resolved as `state.current`)
        // before this function is ever called -- see that loop's own
        // "Winston Churchill's once-per-turn choice" handling, right before
        // its own `Move::EndTurn` application.
        ActionClass::EndTurn => r.try_apply(Move::EndTurn, true),
        // Unreachable: the dispatch loop in `replay_game` special-cases
        // `RemoveLeaderYellow`, `ColumbusColonize`, and `Barbarossa` before
        // any of them ever reaches this function, the same way it special-
        // cases `EndTurn` -- all four are `ActionClass`es whose journal line
        // carries no leading actor colour, so all four need the actor
        // resolved before `apply_one`'s normal `actor` parameter (already
        // committed to by then) would even be correct. `BachTheater` is the
        // one exception among the no-leading-colour classes: its actor is
        // always just `state.current`, so the dispatch loop resolves that
        // trivially and still routes it through this function for real (see
        // its own arm above), rather than stubbing it out here too.
        ActionClass::RemoveLeaderYellow => {
            Err(MismatchKind::ParserGap("RemoveLeaderYellow should have been resolved before apply_one".into()))
        }
        ActionClass::ColumbusColonize => {
            Err(MismatchKind::ParserGap("ColumbusColonize should have been resolved before apply_one".into()))
        }
        ActionClass::Barbarossa => {
            Err(MismatchKind::ParserGap("Barbarossa should have been resolved before apply_one".into()))
        }
    }
}

/// Whether `apply_one`'s dispatch for `class` grounds a named card into
/// `hand_military` and then consumes that identical identity via the `Move`
/// it applies -- i.e. whether `class` is subject to the net-zero hazard
/// [`Replayer::consume_named_military_card`] exists to make unwritable at
/// its own call sites (`docs/REPLAY.md`'s "Discard-phase hand-size oracle"
/// section has the full history: `resolve_political_decision`'s
/// `PrepareEvent` handling had this shape first, then it turned out
/// `PlayTactic`/`DeclareWar`/`PlayAggression`/`ProposePact`/
/// `ColumbusColonize` all had it independently too).
///
/// EXHAUSTIVE, NO WILDCARD ARM ON PURPOSE: a new `ActionClass` variant
/// (`corpus.rs`) fails to compile here until someone decides which side of
/// this list it belongs on -- the mechanism that would have caught the
/// original bug's second, third, fourth and fifth occurrence the day each
/// arm was written, instead of leaving them for a later audit pass to find
/// one at a time. `replay_common::tests::
/// every_card_consuming_action_class_nets_hand_military_down_by_exactly_one`
/// drives its own coverage off this same function.
///
/// Only referenced from `#[cfg(test)]` today (deliberately kept as a plain,
/// always-compiled `fn` rather than moved inside the test module, so the
/// exhaustiveness check itself is not accidentally test-gated away) --
/// `#[allow(dead_code)]` is the honest annotation for a function whose
/// entire purpose is enforcing an invariant `cargo test` checks, not one
/// `replaystats` itself calls at runtime.
#[allow(dead_code)]
fn action_class_grounds_and_consumes_a_card(class: ActionClass) -> bool {
    match class {
        ActionClass::PlayTactic
        | ActionClass::DeclareWar
        | ActionClass::PlayAggression
        | ActionClass::ProposePact
        | ActionClass::ColumbusColonize => true,

        ActionClass::TakeCard
        | ActionClass::BuildBuilding
        | ActionClass::BuildUnit
        | ActionClass::BuildWonderStage
        | ActionClass::IncreasePopulation
        | ActionClass::UpgradeUnit
        | ActionClass::UpgradeProduction
        | ActionClass::DevelopTechnology
        | ActionClass::ElectLeader
        | ActionClass::ChangeGovernment
        | ActionClass::WinWar
        | ActionClass::AcceptPact
        | ActionClass::Colonize
        | ActionClass::Discard
        | ActionClass::Bid
        | ActionClass::WinAuction
        | ActionClass::Destroy
        | ActionClass::Disband
        | ActionClass::Pass
        | ActionClass::PlayEvent
        | ActionClass::PlayActionCard
        | ActionClass::PutBack
        | ActionClass::EndTurn
        | ActionClass::RemoveLeaderYellow
        | ActionClass::Barbarossa
        | ActionClass::BachTheater => false,
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
///   `defense_bonus == b` and grounds-and-commits it in one call
///   (`Replayer::consume_named_military_card` -- the same atomic wrapper
///   every other journal-named play in this file uses; see its own doc for
///   why grounding and consuming must never be two separable steps here).
///   This is not a guess: the age I/II/III bonus cards are the ONLY cards
///   with a nonzero `defense_bonus`, one value each, so the printed number
///   alone is already the card's full identity -- whether or not this
///   binary's fictional simulated hand happened to deal that exact card is
///   irrelevant, same as it is irrelevant for a `PlayAggression`/
///   `DeclareWar`/`ProposePact`/`PlayTactic` line naming a card the
///   simulated deal never dealt.
/// - `Flat`: any currently-legal `Move::Defend` candidate with
///   `defense_bonus == 0` qualifies (every non-`Bonus` military-deck card
///   defends for the same flat +1, `interact::defense_points`). If the
///   simulated hand has one, `r.discard_solver` picks among them exactly as
///   it does for a forced hand-limit discard (same underlying fact: a
///   specific card permanently leaves the hand), so the same solved/
///   chosen/forced-collision honesty applies -- that card is REAL, already
///   in hand, so a plain `try_apply` (no grounding) is correct here. If it
///   has NONE (a small simulated hand can, by chance, be all `Bonus` cards
///   -- seen in the real corpus), [`flat_defense_filler`] grounds-and-
///   commits one through the same atomic wrapper: since identity cannot
///   affect any observable outcome here, this cannot be a wrong guess in
///   the sense the rest of this file guards against, only an arbitrary
///   bookkeeping label.
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
        match clause {
            DefenseClause::Bonus(bonus) => {
                let id = defense_bonus_card(bonus);
                r.consume_named_military_card(player, id, Move::Defend { card: id }, false)?;
            }
            DefenseClause::Flat => {
                let flat: Vec<CardId> = legal::legal_moves(&r.state)
                    .as_slice()
                    .iter()
                    .filter_map(|mv| match mv {
                        Move::Defend { card } if card.get().effects.defense_bonus == 0 => Some(*card),
                        Move::Take { .. } | Move::Build { .. } | Move::Develop { .. } | Move::Upgrade { .. } | Move::WonderStep { .. } | Move::Pop | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => None,
                    })
                    .collect();
                if flat.is_empty() {
                    let filler = flat_defense_filler(r.state.age_military);
                    r.consume_named_military_card(player, filler, Move::Defend { card: filler }, false)?;
                } else {
                    let (idx, certainty) = r.discard_solver.choose(player, r.current_lineno, &flat);
                    if crate::debugflags::replay_debug_all() {
                        eprintln!(
                            "DEBUG aggression defense: player {player} line {} picked {} of {} flat candidates ({certainty:?})",
                            r.current_lineno,
                            flat[idx].get().name,
                            flat.len()
                        );
                    }
                    // Already a REAL card in hand -- nothing to ground.
                    r.try_apply(Move::Defend { card: flat[idx] }, false)?;
                }
            }
        }
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

    /// Minimal `PlayerState`, same field-literal shape `apply.rs`/
    /// `combat.rs`/`costs.rs`/`effects.rs`/`legal.rs`/`economy.rs`/
    /// `events.rs`/`interact.rs`/`advisor/state_io.rs`/`bots/weighted/
    /// events.rs` each already keep their own copy of (see `GameState::
    /// last_end_of_turn_culture`'s own doc for why: no `Default`/spread
    /// pattern in this codebase, so every construction site is a field
    /// literal) -- `replay_common.rs`'s own tests never needed a full
    /// `GameState` before `ground_final_events`.
    fn blank_player(idx: u8) -> PlayerState {
        PlayerState {
            idx,
            techs: crate::state::Tableau::new(),
            government: CardId::NONE,
            leader: CardId::NONE,
            wonder: CardId::NONE,
            wonder_steps: 0,
            completed_wonders: CardList::new(),
            destroyed_wonders: 0,
            homer_wonder: CardId::NONE,
            tactic: CardId::NONE,
            tactic_exclusive: false,
            colonies: CardList::new(),
            flipped_wonders: CardList::new(),
            pacts: crate::state::PactList::new(),
            hand_civil: CardList::<MAX_HAND>::new(),
            hand_military: CardList::<MAX_HAND>::new(),
            hidden_civil: 0,
            hidden_military: 0,
            yellow_bank: 0,
            yellow_granted: 0,
            workers_free: 0,
            raid_loot_pending: 0,
            blue_total: 0,
            food: 0,
            resources: 0,
            science: 0,
            culture: 0,
            culture_rate_extra: 0,
            science_rate_extra: 0,
            strength_extra: 0,
            happy_extra: 0,
            civil_actions: 0,
            military_actions: 0,
            politics_done: false,
            tactic_action_used: false,
            taken_this_turn: CardList::new(),
            ca_spent_taking: 0,
            hammurabi_used: false,
            hammurabi_replaced_this_turn: false,
            breakthrough_ma_funded: false,
            replaced_leader_this_turn: false,
            trade_food_as_resource_used_this_turn: 0,
            trade_resource_as_food_used_this_turn: 0,
            churchill_used: false,
            homer_used_this_turn: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            caesar_second_politics: false,
            peeked_event: CardId::NONE,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: crate::state::OneTimeDiscount::default(),
            resigned: false,
            food_tokens: crate::state::TokenBank::default(),
            resource_tokens: crate::state::TokenBank::default(),
            taken_leader_ages: 0,
            war_declared_by_me: CardId::NONE,
            war_target: 0,
            wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
        }
    }

    fn blank_state() -> GameState {
        GameState {
            num_players: 2,
            seed: 0,
            players: [blank_player(0), blank_player(1), blank_player(2), blank_player(3)],
            current: 0,
            turn: 1,
            round: 1,
            start_player: 0,
            age_civil: crate::cards::Age::A,
            age_military: crate::cards::Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; crate::state::ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: crate::cards::Age::A,
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
            available_tactics: CardList::new(),
            civil_discard: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            civil_removed: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            discarded_military: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            last_round: false,
            final_round_end: None,
            game_over: false,
            phase: Phase::Actions,
            forced_winner: None,
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
            last_end_of_turn_culture: [None; MAX_PLAYERS],
            last_end_of_turn_science: [None; MAX_PLAYERS],
            last_end_of_turn_resources: [None; MAX_PLAYERS],
        }
    }

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// REGRESSION (GAMEDROP.txt's "TruncatedAfterGameOver" bucket -- 68/789
    /// skipped games corpus-wide, one narrow cause, zero exceptions either
    /// direction across all 1011 journals). BGO logs a MINORITY of
    /// journals' duplicated trailing "End turn" line pair -- see the long
    /// comment above the `if line.text.starts_with("Last turn")`/`"End of
    /// game"` handling -- BEFORE their own "End of game" marker instead of
    /// after. Real game `7522663` (2p) is the concrete example this was
    /// traced on: its journal's own last few lines are (line numbers from
    /// `/tmp/bgo-journals/journals/7522663.tsv`)
    ///   397  End turn Purple scores: ...      (real final turn; this is
    ///        the one that genuinely triggers `game::advance_turn`'s
    ///        round-wrap check, so `game::finish_game` fires HERE, off the
    ///        still-fictional event piles)
    ///   398  End turn Purple scores: ...      (BGO's own duplicate copy;
    ///        the top-of-loop `if r.state.game_over { break; }` guard
    ///        trips on this line, one line before the marker below)
    ///   399  No Discard Phase
    ///   400  End of game Check the journal ...
    /// so by the time the guard fires, `state.game_over` is already
    /// correctly `true` but `completed` -- set ONLY by literally reading
    /// line 400's text -- has never had the chance to become `true`, and
    /// the wrong (ungrounded) final-event award from line 397's premature
    /// `finish_game` is already baked into `p.culture`. The `if
    /// !completed { ... }` peek-and-correct block in the `if
    /// r.state.game_over` arm just above is what recovers this: reverting
    /// it (leaving a bare `break;`) reproduces the original bug exactly --
    /// this game replays every move legally, reaches the real end, and is
    /// STILL discarded as "no verified outcome".
    #[test]
    fn replay_game_completes_a_journal_whose_duplicated_end_turn_pair_is_logged_before_its_own_end_of_game_marker() {
        let index_path = format!("{}/../sources/bgo/index.tsv", env!("CARGO_MANIFEST_DIR"));
        let games = match crate::corpus::parse_index(&index_path) {
            Ok(g) => g,
            Err(e) => panic!("{index_path}: {e}"),
        };
        let Some(meta) = games.iter().find(|g| g.id == "7522663") else {
            return; // index.tsv changed on this machine -- nothing to pin, same skip discipline as the fixture below
        };
        let path = "/tmp/bgo-journals/journals/7522663.tsv";
        let Ok(text) = std::fs::read_to_string(path) else {
            return; // journals dir not extracted on this machine (tar -xzf sources/bgo/journals.tar.gz -C /tmp/bgo-journals)
        };
        // Confirm the fixture still has the exact shape this regression
        // depends on (the marker strictly after the last "End turn" line),
        // rather than silently testing nothing if the corpus ever changes.
        let eog_lineno = text.lines().position(|l| l.contains("End of game"));
        let last_end_turn_lineno = text.lines().enumerate().filter(|(_, l)| l.contains("End turn")).map(|(i, _)| i).last();
        match (eog_lineno, last_end_turn_lineno) {
            (Some(eog), Some(let_)) => assert!(
                eog > let_,
                "game 7522663 no longer exhibits the trailing-line-order quirk (End of game at {eog}, last End turn at {let_}) -- picked the wrong fixture"
            ),
            _ => panic!("game 7522663's journal is missing an End of game or End turn line -- picked the wrong fixture"),
        }

        let card_index = build_card_index();
        let result = replay_game(meta, &text, &card_index, false);

        assert!(
            result.mismatch.is_none(),
            "expected a clean replay, got a mismatch at line {}: {:?}",
            result.mismatch.as_ref().map_or(0, |m| m.lineno),
            result.mismatch.as_ref().map(|m| &m.raw_text)
        );
        assert!(
            result.completed,
            "game 7522663 has a real End of game marker after its duplicated End turn pair -- must complete, not be discarded"
        );
        assert!(result.engine_scores.is_some(), "a completed game must have grounded final scores");
    }

    /// BGO logs the SAME end-of-turn discard TWICE at the same second: the
    /// player's own `End turn ... draws N military cards` line, then a
    /// trailing `<Color> discards N card(s)` line. The `EndTurn` arm's
    /// `try_apply(Move::EndTurn)` already advanced `state.current` to the
    /// NEXT player by the time the trailing line is reached, so
    /// `state.decider() != actor` for it -- the one field that distinguishes
    /// the redundant copy from a genuine resolution (a `Discard Phase` line
    /// is reached while `state.current` is STILL the acting player, so
    /// `decider() == actor` there and is never skipped). Skipping the
    /// trailing line's `resolve_intervening` is what lets the replay
    /// continue instead of reporting `StuckPending: decider != expected
    /// actor, phase Actions, no pending` for 89 of the ~116 games in that
    /// bucket. Pinned to real game `7523354` (2p), whose line 251
    /// `"Purple discards 1 card"` at `02:18:02` follows its own `End turn
    /// Purple` line 250 at the same second.
    #[test]
    fn replay_game_skips_the_redundant_trailing_discard_line_after_an_end_turn() {
        let index_path = format!("{}/../sources/bgo/index.tsv", env!("CARGO_MANIFEST_DIR"));
        let games = match crate::corpus::parse_index(&index_path) {
            Ok(g) => g,
            Err(e) => panic!("{index_path}: {e}"),
        };
        let Some(meta) = games.iter().find(|g| g.id == "7523354") else {
            return; // index.tsv changed on this machine -- nothing to pin
        };
        let path = "/tmp/bgo-journals/journals/7523354.tsv";
        let Ok(text) = std::fs::read_to_string(path) else {
            return; // journals dir not extracted on this machine
        };
        // Confirm the fixture still has the exact shape this regression
        // depends on: a `discards` line whose immediately-preceding line is
        // an `End turn` for the SAME player (the redundant same-second copy).
        // The journal rows are 5 tab-separated columns (date, colour, age,
        // round, text), so the `End turn` text lives in the 5th column, not
        // at the row's start.
        let lines: Vec<&str> = text.lines().collect();
        let has_trailing_pair = lines.iter().enumerate().any(|(k, l)| {
            l.contains("discards")
                && k > 0
                && lines[k - 1].split('\t').nth(4).is_some_and(|t| t.starts_with("End turn"))
        });
        assert!(
            has_trailing_pair,
            "game 7523354 no longer exhibits the redundant trailing-discard pair -- picked the wrong fixture"
        );

        let card_index = build_card_index();
        let result = replay_game(meta, &text, &card_index, false);

        assert!(
            result.mismatch.is_none(),
            "expected a clean replay, got a mismatch at line {}: {:?}",
            result.mismatch.as_ref().map_or(0, |m| m.lineno),
            result.mismatch.as_ref().map(|m| &m.raw_text)
        );
        assert!(result.completed, "game 7523354 must complete, not stop on the trailing discard");
        assert!(result.trailing_discards > 0, "at least one redundant trailing discard line must be skipped");
        assert!(result.engine_scores.is_some(), "a completed game must have grounded final scores");
    }

    /// The real shape of `sources/bgo` journal `7522625`'s own final line
    /// (`docs/REPLAY.md`'s "Final scores" section): the four real pending
    /// cards sit between "Check the journal..." and the "; WINNER IS..."
    /// tail, which must NOT be swept in even though it also contains
    /// semicolon-separated clauses.
    #[test]
    fn parse_real_final_events_extracts_the_impact_of_clauses_and_stops_before_winner_is() {
        let card_index = build_card_index();
        let text = "End of game Check the journal to get the final impacts effects :; Impact of Balance; Impact \
                     of Progress; Impact of Happiness; Impact of Architecture; ; WINNER IS RICARDO LOPEZ ANTON AS \
                     ORANGE (177 PTS); 2nd is PLAYER as Purple (165 pts)";
        let got = parse_real_final_events(text, &card_index);
        assert_eq!(
            got,
            vec![
                card("Impact of Balance"),
                card("Impact of Progress"),
                card("Impact of Happiness"),
                card("Impact of Architecture"),
            ]
        );
    }

    /// The SAME card can be announced TWICE in one marker line: BGO logs
    /// the line per seat, and a journal whose trigger seat's copy is not
    /// the first one (7523253: Grey's copy precedes Orange's) reaches the
    /// "End of game" marker with `game_over` still false and a line whose
    /// pending SET is duplicated. The announced SET is a set, so the
    /// parse must keep one entry per card, in order of first appearance --
    /// otherwise `ground_final_events` plants the duplicate in
    /// `future_events` and `evaluate_final_events` scores that card twice.
    #[test]
    fn parse_real_final_events_keeps_one_entry_per_card_when_the_line_announces_one_twice() {
        let card_index = build_card_index();
        let text = "End of game Check the journal to get the final impacts effects :; Impact of Progress; \
                    Impact of Progress; ; WINNER IS PIOTR AS ORANGE (240 PTS); 2nd is X as Purple (199 pts)";
        assert_eq!(
            parse_real_final_events(text, &card_index),
            vec![card("Impact of Progress")]
        );
    }

    /// The other real shape (10/1,011 games in the corpus): no cards were
    /// still pending, so BGO's own line has no "Check the journal" preamble
    /// and no `"Impact of"` clauses at all -- must resolve to empty, not a
    /// parse error, since an empty final-event set is a legitimate real
    /// outcome (every scoringEvent card got drawn and resolved mid-game).
    #[test]
    fn parse_real_final_events_is_empty_when_the_real_game_had_no_cards_left_pending() {
        let card_index = build_card_index();
        let text = "End of game ; WINNER IS PLAYER AS ORANGE (102 PTS); 2nd is PLAYER2 as Purple (48 pts)";
        assert_eq!(parse_real_final_events(text, &card_index), Vec::<CardId>::new());
    }

    /// This is the fix itself: `events::evaluate_final_events` reads
    /// `current_events`/`future_events` (via `pending_final_events`), and
    /// before this fix those piles held whatever the replayer's own
    /// event-plan reconstruction guessed for cards the real game never
    /// revealed -- a fictional set `event_plan`'s own module doc already
    /// admits "nothing ever validates". Confirmed RED by reverting this
    /// function to a no-op: the assertion below fails
    /// `left: [Vast Territory (II), Impact of Industry], right: [Impact of
    /// Balance, Impact of Progress]` (the untouched fictional pile survives
    /// instead of being replaced).
    #[test]
    fn ground_final_events_replaces_both_piles_with_exactly_the_real_cards_dropping_any_fictional_leftovers() {
        let mut state = blank_state();
        // A fictional pile this reconstruction's own event-plan machinery
        // might plausibly have left behind -- one non-scoring Territory
        // card (irrelevant to final scoring either way) and one WRONG
        // scoringEvent card that must be dropped, not merged with the real
        // set.
        state.current_events.push(card("Vast Territory (II)"));
        state.current_events.push(card("Impact of Industry"));
        let real = vec![card("Impact of Balance"), card("Impact of Progress")];
        ground_final_events(&mut state, &real);
        assert!(state.current_events.is_empty());
        assert_eq!(state.future_events.as_slice(), real.as_slice());
    }

    #[test]
    fn parse_age_reads_every_roman_numeral_the_journal_actually_prints() {
        assert_eq!(parse_age("A"), Some(crate::cards::Age::A));
        assert_eq!(parse_age("I"), Some(crate::cards::Age::I));
        assert_eq!(parse_age("II"), Some(crate::cards::Age::II));
        assert_eq!(parse_age("III"), Some(crate::cards::Age::III));
        assert_eq!(parse_age("IV"), Some(crate::cards::Age::IV));
    }

    #[test]
    fn parse_age_is_none_for_the_header_rows_own_literal_column_name() {
        // `parse_lines` already drops the header line before any `Line` is
        // built, but this stays a defined `None` rather than a guess in
        // case a future caller ever feeds it raw, unfiltered text.
        assert_eq!(parse_age("age"), None);
        assert_eq!(parse_age(""), None);
    }

    fn line<'a>(lineno: usize, age: &'a str, text: &'a str) -> Line<'a> {
        Line { lineno, color: "Orange", age, round: "1", text }
    }

    /// The exact shape traced on real game `7523818` line 8 (`docs/REPLAY.md`
    /// "civil deck model" handoff): BGO logs the NEXT player's own "Action
    /// Phase begins" marker (already tagged the new age) BEFORE the
    /// PREVIOUS player's own trailing "End turn ... scores: ..." line for
    /// the round that just ended (still, correctly, tagged the OLD age).
    /// `last_real_decision_line_for_age`'s job is to see PAST that stale
    /// trailer -- the last REAL decision still tagged "A" is the `Take` at
    /// index 0, not the `EndTurn` at index 2.
    #[test]
    fn last_real_decision_line_for_age_ignores_an_end_turn_trailer_still_tagged_the_old_age() {
        let card_index = build_card_index();
        let journal = [
            line(2, "A", "Purple takes Pyramids in hand Purple uses 2 civil action"),
            line(3, "I", "Action Phase begins"),
            line(4, "A", "End turn Purple scores:; ; 0 culture (now 0)"),
            line(5, "A", "No Discard Phase"),
        ];
        let last = last_real_decision_line_for_age(&journal, &card_index);
        assert_eq!(last[crate::cards::Age::A as usize], Some(0), "the Take, not the later-indexed EndTurn trailer");
        assert_eq!(last[crate::cards::Age::I as usize], None, "\"Action Phase begins\" is Bookkeeping, not a decision");
    }

    /// The exact shape traced on real game `7522652` line 430: BGO logs
    /// both players' `"Last turn Game ends..."` §12.3 notices (already
    /// tagged the new age `IV`) BEFORE Green's own `"discards N cards"`
    /// line resolving the outstanding military discard that finishes
    /// Green's end of turn -- still, correctly, tagged the OLD age `III`.
    /// `ActionClass::Discard`'s own doc (`apply_one`) is explicit that
    /// resolving the last queued discard can itself trigger the real
    /// transition, so this line is exactly as much a wrap-up trailer as an
    /// `EndTurn` line, not a fresh mid-age decision.
    #[test]
    fn last_real_decision_line_for_age_ignores_a_discard_resolution_trailer_still_tagged_the_old_age() {
        let card_index = build_card_index();
        let journal = [
            line(2, "III", "Purple takes Pyramids in hand Purple uses 2 civil action"),
            line(3, "IV", "Last turn Game ends at the end of the starting round"),
            line(4, "III", "Green discards 2 cards"),
        ];
        let last = last_real_decision_line_for_age(&journal, &card_index);
        assert_eq!(last[crate::cards::Age::III as usize], Some(0), "the Take, not the later-indexed Discard trailer");
    }

    /// A real, non-wrap-up decision genuinely still tagged the OLD age
    /// AFTER this reconstruction has already moved on IS the bug this
    /// instrument exists to catch -- confirms the exclusion above is
    /// narrow (only `EndTurn`/`Discard`), not a blanket "ignore anything
    /// after the first new-age line" that would also hide a real
    /// divergence.
    #[test]
    fn last_real_decision_line_for_age_still_sees_a_real_decision_tagged_the_old_age() {
        let card_index = build_card_index();
        let journal = [
            line(2, "A", "Purple takes Pyramids in hand Purple uses 2 civil action"),
            line(3, "I", "Orange takes Hammurabi in hand Orange uses 1 civil action"),
            line(4, "A", "Purple takes Colossus in hand Purple uses 3 civil action"),
        ];
        let last = last_real_decision_line_for_age(&journal, &card_index);
        assert_eq!(last[crate::cards::Age::A as usize], Some(2));
    }

    /// The `IllegalMove: Take` `Budget` bug this pass fixed (game `7522905`
    /// line 17-19): Frugality (A) has TWO physical copies in the Age A deck
    /// (`count: [2, 2, 2]`, `costs::can_take_gated`'s own doc -- action
    /// cards are exempt from the one-per-name rule so several copies can sit
    /// in the row and be held at once), so a player can genuinely have two
    /// OPEN takes of the identically-named "Frugality" at once, at two
    /// different row costs. The putback here refunds 2 civil actions,
    /// matching the FIRST take's cost, not the second (most recent) one --
    /// popping "most recent" would wrongly skip the 1-CA take and leave the
    /// 2-CA take as the "real" one, overcounting this turn's true spend by 1
    /// CA and manufacturing a Budget rejection on a later, actually
    /// affordable, take this same turn.
    #[test]
    fn prescan_putback_skips_matches_a_putback_to_the_take_its_refund_actually_costs_not_the_most_recent_one() {
        let card_index = build_card_index();
        let journal = [
            line(16, "A", "Grey takes Rich Land in hand Grey uses 1 civil action"),
            line(17, "A", "Grey takes Frugality in hand Grey uses 2 civil action"),
            line(18, "A", "Grey takes Frugality in hand Grey uses 1 civil action"),
            line(19, "A", "Grey puts Frugality back in the row Grey gets 2 civil action"),
            line(20, "A", "Grey takes Urban Growth in hand Grey uses 2 civil action"),
        ];
        let skip = prescan_putback_skips(&journal, &card_index);
        assert!(
            skip.contains(&1) && skip.contains(&3),
            "the 2-CA take (index 1) and the putback (index 3) are the pair the refund names -- got {skip:?}"
        );
        assert!(
            !skip.contains(&2),
            "the 1-CA take (index 2) must survive as Grey's real, kept Frugality -- got {skip:?}"
        );
    }

    /// With only ONE open take of a given card, the refund-matching above
    /// must be a no-op over the original "most recent" behaviour --
    /// confirms the fix does not regress the overwhelmingly common,
    /// single-copy case.
    #[test]
    fn prescan_putback_skips_still_pairs_a_lone_take_regardless_of_its_own_refund_amount() {
        let card_index = build_card_index();
        let journal = [
            line(1, "A", "Purple takes Pyramids in hand Purple uses 3 civil action"),
            line(2, "A", "Purple puts Pyramids back in the row Purple gets 3 civil action"),
        ];
        let skip = prescan_putback_skips(&journal, &card_index);
        assert_eq!(skip, [0, 1].into_iter().collect(), "the only open take pairs with the putback even with one candidate");
    }

    /// The `UnrecoverableHiddenInfo: unpaired BGO client-side undo` bug this
    /// pass fixed (game `7522613` line 177/216/217/219): Green takes
    /// Reserves (I) (`count [2,2,2]`) for 2 CA in round 6, takes a SECOND
    /// Reserves (I) for 1 CA in round 7, PLAYS a Reserves (round 7,
    /// resolving its one-time effect), then puts one back for a 1 CA
    /// refund. The old `stack.retain(|a| a != actor)` on the `plays` line
    /// dropped BOTH still-open takes (right for the single-copy case,
    /// where there is only ever one to drop; wrong here, where a second,
    /// unrelated take from an earlier turn was sitting in the same
    /// per-card stack) -- so the later putback found an EMPTY stack and
    /// reported unpaired outright, not merely a wrong pairing. The
    /// putback's own 1 CA refund is direct evidence the round-7 take (also
    /// 1 CA) is the one that survived to be put back, so the commit must
    /// have consumed the OLDER round-6 take -- i.e. a commit should pop
    /// only the OLDEST open take (FIFO), not wipe every open take.
    #[test]
    fn prescan_putback_skips_lets_a_commit_consume_only_the_oldest_open_take_not_every_open_take() {
        let card_index = build_card_index();
        let journal = [
            line(177, "I", "Green takes Reserves in hand Green uses 2 civil action"),
            line(216, "I", "Green takes Reserves in hand Green uses 1 civil action"),
            line(217, "I", "Green plays Reserves Green produces 2 resources"),
            line(219, "I", "Green puts Reserves back in the row Green gets 1 civil action"),
        ];
        let skip = prescan_putback_skips(&journal, &card_index);
        assert!(
            skip.contains(&1) && skip.contains(&3),
            "the round-7 take (index 1) and the putback (index 3) are the pair the refund names -- got {skip:?}"
        );
        assert!(
            !skip.contains(&0),
            "the round-6 take (index 0) was consumed by the `plays` line, not put back, and must stay a real applied move -- got {skip:?}"
        );
    }

    /// Companion to the above with only ONE open take at the time of the
    /// commit: popping "the oldest" must degrade to the original
    /// behaviour (removing that single entry), confirming the fix does
    /// not regress the overwhelmingly common single-copy case.
    #[test]
    fn prescan_putback_skips_still_drops_a_lone_take_on_commit() {
        let card_index = build_card_index();
        let journal = [
            line(1, "A", "Orange takes Frugality in hand Orange uses 1 civil action"),
            line(2, "A", "Orange plays Frugality Orange increases population; Orange spends 2 food; Orange produces 1 food"),
            line(3, "A", "Orange puts Frugality back in the row Orange gets 1 civil action"),
        ];
        let skip = prescan_putback_skips(&journal, &card_index);
        assert!(
            skip.is_empty(),
            "the take was consumed by the `plays` line, so the later, unrelated putback has nothing open to pair with -- got {skip:?}"
        );
    }

    /// The `IllegalMove: Pop` bug this pass fixed (game `7522648` round 7,
    /// `docs/REPLAY.md`'s handoff): a player's own `End turn` line can leave
    /// a `DiscardMilitary` choice open (`economy::end_of_turn` interrupted
    /// before its own production/consumption steps), and the very NEXT
    /// journal line is routinely already tagged the new age. Catching the
    /// age up right then would apply §12.2.4's "-2 yellow_bank" deduction
    /// to a player whose OWN end-of-turn consumption for the OLD age has not
    /// run yet, back-dating the deduction onto their still-in-progress turn.
    #[test]
    fn catch_up_civil_age_defers_while_a_discard_choice_from_an_earlier_line_is_still_open() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.age_civil = crate::cards::Age::I;
        let yellow_before = (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank);

        let mut options = crate::state::OptionList::new();
        let legion = CardId::by_name("Legion").expect("Legion is a known military card");
        options.push(ChoiceOption::Card(legion));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::DiscardMilitary, options }));

        catch_up_civil_age(&mut r.state, "II");

        assert_eq!(r.state.age_civil, crate::cards::Age::I, "must not advance while the discard is still open");
        assert_eq!(
            (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank),
            yellow_before,
            "§12.2.4's deduction must not fire before the interrupted end_of_turn resumes"
        );
    }

    /// The other half of the fix above: once nothing is outstanding, the
    /// SAME call still catches the age up (and runs §12.2.4's deduction) --
    /// this is a deferral, not a skip.
    #[test]
    fn catch_up_civil_age_advances_once_pending_and_queue_are_both_empty() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.age_civil = crate::cards::Age::I;
        assert!(r.state.pending.is_empty());
        assert!(r.state.queue.is_empty());
        let yellow_before = r.state.players[0].yellow_bank;

        catch_up_civil_age(&mut r.state, "II");

        assert_eq!(r.state.age_civil, crate::cards::Age::II);
        assert_eq!(r.state.players[0].yellow_bank, yellow_before - 2, "§12.2.4");
    }

    /// Game `7522064`'s own bug, distinct from the discard-choice shape
    /// above: `catch_up_civil_age`'s `pending`/`queue` guard only catches a
    /// turn caught MID-interruption. Line 328's "Last turn" trailer sits
    /// ahead of a DIFFERENT player's own still-fully-synchronous (nothing
    /// outstanding yet -- her `End turn` line simply has not been REACHED)
    /// `End turn`/`discards` pair two lines later, still tagged the OLD age.
    /// `replay_game`'s main loop closes this with the
    /// `is_trustworthy_age_line(classify(...))` gate around its call to
    /// `catch_up_civil_age` -- this test reproduces that exact gated
    /// snippet directly. Reverting the gate (calling `catch_up_civil_age`
    /// unconditionally, as the loop used to) turns the first assertion RED:
    /// the "Last turn" line alone would force Age III -> IV and dock both
    /// players' `yellow_bank` a whole turn before Purple's own round-17
    /// production/consumption -- the exact desync that undercounted her
    /// food by 1 and made a legal `Pop` look illegal.
    #[test]
    fn catch_up_civil_age_call_site_gate_ignores_a_last_turn_line_but_still_advances_on_the_next_real_decision() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.age_civil = crate::cards::Age::III;
        assert!(r.state.pending.is_empty());
        assert!(r.state.queue.is_empty());
        let yellow_before = (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank);

        // The exact call-site snippet in `replay_game`'s main loop, for a
        // BGO "Last turn" trailer (Bookkeeping, tagged the NEW age).
        let last_turn = "Last turn Game ends at the end of the starting round";
        if is_trustworthy_age_line(classify(&card_index, last_turn)) {
            catch_up_civil_age(&mut r.state, "IV");
        }
        assert_eq!(r.state.age_civil, crate::cards::Age::III, "a Last turn trailer must not force the age");
        assert_eq!(
            (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank),
            yellow_before,
            "§12.2.4 must not fire off an untrustworthy line"
        );

        // The SAME snippet, now for the next REAL decision (still tagged
        // the new age) -- the age must still advance, or it would never
        // advance at all.
        let real_decision = "Purple takes Pyramids in hand Purple uses 2 civil action";
        if is_trustworthy_age_line(classify(&card_index, real_decision)) {
            catch_up_civil_age(&mut r.state, "IV");
        }
        assert_eq!(r.state.age_civil, crate::cards::Age::IV, "a genuine decision line must still force the age forward");
        assert_eq!(
            r.state.players[0].yellow_bank,
            yellow_before.0 - 2,
            "§12.2.4's deduction runs once the age is genuinely forced"
        );
    }

    /// The exact shape traced on real game `7523079` line 210 (culture-
    /// oracle `WinWar` bucket, Purple's round-12 `End turn` at line 211:
    /// journal `(now 73)`, this binary `61`, delta -12): Orange's own
    /// `"wins War over Territory"` confirmation line is tagged the
    /// ATTACKER's own NEXT age (`III`, where `game::start_turn` actually
    /// resolves the war -- `is_pure_confirmation_line`'s own doc), but BGO
    /// prints it (same timestamp) one line BEFORE Purple's still-Age-`II`
    /// `End turn`. Before this fix, `is_trustworthy_age_line` excluded only
    /// `EndTurn`/`Discard`, so this line's `III` tag forced `state.age_civil`
    /// up a whole turn early and ran §12.2.4's "-2 yellow_bank" deduction on
    /// BOTH players before Purple's own end-of-turn ran -- `happy_required`'s
    /// band jumped off the prematurely-decremented bank, a false `uprising`
    /// (§6.3) tripped, and `economy::end_of_turn` skipped ALL of step 3
    /// (culture/science/corruption/food/resources) for a turn BGO's journal
    /// shows producing normally. Mirrors
    /// `catch_up_civil_age_call_site_gate_ignores_a_last_turn_line_but_
    /// still_advances_on_the_next_real_decision`'s own shape one line up.
    #[test]
    fn is_trustworthy_age_line_call_site_gate_ignores_a_winwar_confirmation_but_still_advances_on_the_next_real_decision() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.age_civil = crate::cards::Age::II;
        assert!(r.state.pending.is_empty());
        assert!(r.state.queue.is_empty());
        let yellow_before = (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank);

        // The exact call-site snippet in `replay_game`'s main loop, for a
        // BGO WinWar confirmation line (a `is_pure_confirmation_line` class,
        // tagged the attacker's NEW age).
        let win_war = "Orange wins War over Territory Attacker's strength: 25; Defender's strength: 11";
        if is_trustworthy_age_line(classify(&card_index, win_war)) {
            catch_up_civil_age(&mut r.state, "III");
        }
        assert_eq!(r.state.age_civil, crate::cards::Age::II, "a WinWar confirmation line must not force the age");
        assert_eq!(
            (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank),
            yellow_before,
            "§12.2.4 must not fire off an untrustworthy WinWar line"
        );

        // The SAME snippet, now for the next REAL decision (still tagged the
        // new age) -- the age must still advance, or it would never advance
        // at all.
        let real_decision = "Orange plays Raid against Purple Destroy up to 2 of your rival's urban buildings";
        if is_trustworthy_age_line(classify(&card_index, real_decision)) {
            catch_up_civil_age(&mut r.state, "III");
        }
        assert_eq!(r.state.age_civil, crate::cards::Age::III, "a genuine decision line must still force the age forward");
        assert_eq!(
            (r.state.players[0].yellow_bank, r.state.players[1].yellow_bank),
            (yellow_before.0 - 2, yellow_before.1 - 2),
            "§12.2.4's deduction runs once the age is genuinely forced"
        );
    }

    /// The (-6,+6) War-over-Culture cluster's own real shape, real game
    /// `7522590` lines 315-319: the DEFENDER's own `EndTurn` (still Age
    /// III) is followed by a `"No Discard Phase"` bridge line (also III),
    /// THEN the `WinWar` confirmation (tagged IV). Reverting the fix (no
    /// call to `upcoming_confirmed_winwar_age` in `replay_game`'s main
    /// loop) leaves `state.age_civil` at III when `resolve_war_outcome`
    /// actually runs, one line before this function would ever see the
    /// `WinWar` line at all -- exactly the bug.
    #[test]
    fn upcoming_confirmed_winwar_age_finds_the_bridge_across_a_no_discard_phase_trailer() {
        let card_index = build_card_index();
        let journal = [
            line(317, "III", "End turn Orange scores:; ; 7 culture (now 78)"),
            line(318, "III", "No Discard Phase"),
            line(319, "IV", "Purple wins War over Culture Attacker's strength: 40; Defender's strength: 27"),
            line(320, "IV", "Purple plays Armed Intervention against Orange"),
        ];
        assert_eq!(upcoming_confirmed_winwar_age(&journal, 0, &card_index), Some(crate::cards::Age::IV));
        // Starting from the bridge line itself must find the identical
        // answer -- `replay_game`'s loop calls this every line, not just
        // the first one of the bridge.
        assert_eq!(upcoming_confirmed_winwar_age(&journal, 1, &card_index), Some(crate::cards::Age::IV));
    }

    /// A genuine real decision (not a bridge line) between `i` and any
    /// `WinWar` means there is nothing to trust -- this must not scan past
    /// an unrelated player's own ordinary turn looking for some LATER,
    /// unconnected war's confirmation.
    #[test]
    fn upcoming_confirmed_winwar_age_is_none_when_a_real_decision_comes_before_any_winwar() {
        let card_index = build_card_index();
        let journal = [
            line(1, "III", "End turn Orange scores:; ; 7 culture (now 78)"),
            line(2, "III", "Purple takes Pyramids in hand Purple uses 2 civil action"),
            line(3, "IV", "Purple wins War over Culture Attacker's strength: 40; Defender's strength: 27"),
        ];
        assert_eq!(upcoming_confirmed_winwar_age(&journal, 0, &card_index), None);
    }

    /// The journal ending right after the bridge (no `WinWar` ever found)
    /// is the same "nothing to trust" answer, not a panic.
    #[test]
    fn upcoming_confirmed_winwar_age_is_none_when_the_journal_ends_inside_the_bridge() {
        let card_index = build_card_index();
        let journal = [line(1, "III", "End turn Orange scores:; ; 7 culture (now 78)"), line(2, "III", "No Discard Phase")];
        assert_eq!(upcoming_confirmed_winwar_age(&journal, 0, &card_index), None);
    }

    /// The discriminator this function deliberately does NOT apply, and
    /// why: real game `7523045` line 335 has a `WinWar` (Purple, IV)
    /// immediately followed by Orange's own still-Age-III `End turn`/
    /// discard trailer -- the identical "older age immediately after
    /// WinWar" shape `is_trustworthy_age_line`'s own WinWar exclusion
    /// exists to refuse (game `7523079`). This function trusts it anyway --
    /// see its own doc for why that is safe here specifically (its only
    /// caller never touches cross-player state, unlike
    /// `is_trustworthy_age_line`'s caller). Confirmed against the full
    /// 687-game guard corpus with zero regressions.
    #[test]
    fn upcoming_confirmed_winwar_age_trusts_a_winwar_even_when_an_older_tagged_line_follows_it() {
        let card_index = build_card_index();
        let journal = [
            line(334, "III", "Discard Phase 2 military cards must be discarded"),
            line(335, "IV", "Purple wins War over Culture Attacker's strength: 41; Defender's strength: 27"),
            line(336, "III", "End turn Orange scores:; ; 9 culture (now 106)"),
            line(337, "III", "Orange discards 2 cards"),
        ];
        assert_eq!(upcoming_confirmed_winwar_age(&journal, 0, &card_index), Some(crate::cards::Age::IV));
    }

    /// End-to-end wiring: the exact call-site snippet in `replay_game`'s
    /// main loop, using `upcoming_confirmed_winwar_age`'s own real-game-
    /// shaped fixture above. Reverting the fix (deleting the call to
    /// `game::antiquate_leader_wonder_pacts_up_to` at the call site) turns
    /// this RED: Napoleon stays in play at the moment the war resolves.
    #[test]
    fn the_call_site_antiquates_a_too_old_leader_off_an_upcoming_winwar_without_moving_the_age() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.age_civil = crate::cards::Age::III;
        r.state.players[0].leader = CardId::by_name("Napoleon Bonaparte").expect("known leader");
        let yellow_before = r.state.players[0].yellow_bank;

        let journal = [
            line(317, "III", "End turn Orange scores:; ; 7 culture (now 78)"),
            line(318, "III", "No Discard Phase"),
            line(319, "IV", "Purple wins War over Culture Attacker's strength: 40; Defender's strength: 27"),
        ];
        // The exact call-site snippet in `replay_game`'s main loop.
        if let Some(age) = upcoming_confirmed_winwar_age(&journal, 0, &card_index) {
            crate::game::antiquate_leader_wonder_pacts_up_to(&mut r.state, age);
        }

        assert!(r.state.players[0].leader.is_none(), "Napoleon must be gone before the synchronous war resolution reads him");
        assert_eq!(r.state.age_civil, crate::cards::Age::III, "the age itself stays on the normal, deferred schedule");
        assert_eq!(r.state.players[0].yellow_bank, yellow_before, "§12.2.4's deduction must not fire early");
    }

    /// The SAME "`economy::end_of_turn` interrupted before production" shape
    /// as `catch_up_civil_age_defers_while_a_discard_choice_from_an_earlier_
    /// line_is_still_open` above, now for the culture-oracle instrument
    /// (real game `7523350` round 5, `docs/REPLAY.md`'s "Culture-oracle"
    /// section): comparing `state.players[actor].culture` against BGO's own
    /// `"(now M)"` the INSTANT an `EndTurn` line is read is a false
    /// positive whenever that same `EndTurn` opened a `DiscardMilitary`
    /// pending -- production has not run yet. `flush_pending_culture_check`
    /// must defer (not compare) while that pending is still open for the
    /// SAME actor the check belongs to.
    #[test]
    fn flush_pending_culture_check_defers_while_the_actors_own_discard_choice_is_still_open() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let mut options = crate::state::OptionList::new();
        let legion = CardId::by_name("Legion").expect("Legion is a known military card");
        options.push(ChoiceOption::Card(legion));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::DiscardMilitary, options }));
        // Reconstructed PRE-production culture (2), same as BGO's own PRE-
        // production total would be if production had already run -- but it
        // has not, so `journal_now: 3` (BGO's real post-production "(now 3)")
        // must NOT be compared against this yet.
        r.state.players[0].culture = 2;
        r.pending_culture_check =
            Some(PendingCultureCheck { lineno: 64, actor_seat: 0, journal_now: 3, last_action_class: Some(ActionClass::TakeCard) });

        r.flush_pending_culture_check();

        assert_eq!(r.culture_oracle_checked, 0, "must not count a checkpoint whose production has not run yet");
        assert!(r.culture_oracle_divergence.is_none(), "must not flag a false divergence while still blocked");
        assert!(r.pending_culture_check.is_some(), "the check must be put back, not dropped, while still blocked");
    }

    /// The other half: once the actor's `DiscardMilitary` pending is gone
    /// (the resolving `"<Color> discards N card(s)"` line has run and
    /// `game::resume_end_turn` snapshotted the post-production total into
    /// `state.last_end_of_turn_culture[0]`, exactly as it does for real),
    /// the SAME deferred check now compares correctly -- a deferral, not a
    /// permanent skip.
    #[test]
    fn flush_pending_culture_check_compares_once_the_discard_resolves() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        assert!(r.state.pending.is_empty(), "fixture assumption: the discard already resolved, nothing left open");
        r.state.last_end_of_turn_culture[0] = Some(3); // resume_end_turn's own snapshot, matching BGO's own "(now 3)"
        r.pending_culture_check =
            Some(PendingCultureCheck { lineno: 64, actor_seat: 0, journal_now: 3, last_action_class: Some(ActionClass::TakeCard) });

        r.flush_pending_culture_check();

        assert_eq!(r.culture_oracle_checked, 1, "the deferred checkpoint must be counted once resolved");
        assert_eq!(r.culture_oracle_agreed, 1, "3 == 3: this binary's reconstruction agrees with the journal");
        assert!(r.culture_oracle_divergence.is_none());
        assert!(r.pending_culture_check.is_none(), "consumed, not left pending forever");
        assert!(r.state.last_end_of_turn_culture[0].is_none(), "the snapshot must be consumed, not left to leak into a later checkpoint");
    }

    /// [`flush_pending_culture_check_defers_while_the_actors_own_discard_choice_is_still_open`]'s
    /// science twin -- SCIDRIFT 2026-08-14 pass. Same false-positive shape:
    /// production has not run yet, so the pre-production live `science` must
    /// not be compared against BGO's post-production `"(now M)"`.
    #[test]
    fn flush_pending_science_check_defers_while_the_actors_own_discard_choice_is_still_open() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let mut options = crate::state::OptionList::new();
        let legion = CardId::by_name("Legion").expect("Legion is a known military card");
        options.push(ChoiceOption::Card(legion));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::DiscardMilitary, options }));
        r.state.players[0].science = 2; // pre-production, must not be compared yet
        r.pending_science_check = Some(PendingScienceCheck {
            lineno: 64,
            round: "9".to_string(),
            actor_seat: 0,
            journal_now: 6,
            last_action_class: Some(ActionClass::TakeCard),
        });

        r.flush_pending_science_check();

        assert_eq!(r.science_oracle_checked, 0, "must not count a checkpoint whose production has not run yet");
        assert!(r.science_oracle_divergence.is_none(), "must not flag a false divergence while still blocked");
        assert!(r.pending_science_check.is_some(), "the check must be put back, not dropped, while still blocked");
    }

    /// [`flush_pending_culture_check_compares_once_the_discard_resolves`]'s
    /// science twin.
    #[test]
    fn flush_pending_science_check_compares_once_the_discard_resolves() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        assert!(r.state.pending.is_empty(), "fixture assumption: the discard already resolved, nothing left open");
        r.state.last_end_of_turn_science[0] = Some(6); // resume_end_turn's own snapshot, matching BGO's own "(now 6)"
        r.pending_science_check = Some(PendingScienceCheck {
            lineno: 296,
            round: "9".to_string(),
            actor_seat: 0,
            journal_now: 6,
            last_action_class: Some(ActionClass::TakeCard),
        });

        r.flush_pending_science_check();

        assert_eq!(r.science_oracle_checked, 1, "the deferred checkpoint must be counted once resolved");
        assert_eq!(r.science_oracle_agreed, 1, "6 == 6: this binary's reconstruction agrees with the journal");
        assert!(r.science_oracle_divergence.is_none());
        assert!(r.pending_science_check.is_none(), "consumed, not left pending forever");
        assert!(r.state.last_end_of_turn_science[0].is_none(), "the snapshot must be consumed, not left to leak into a later checkpoint");
    }

    /// A real divergence must be recorded, not silently swallowed --
    /// reproduces the exact shape PACTS's writeup found in game `7523162`
    /// (Grey's tracked science silently drops 2 points with no journal
    /// cause): `record_science_check` must set `science_oracle_divergence`
    /// with the right sign (`reconstructed - journal_now`) when the
    /// snapshot disagrees with BGO's own running total.
    #[test]
    fn record_science_check_flags_a_real_divergence_with_the_correct_sign() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.last_end_of_turn_science[0] = Some(4); // this binary's own reconstruction: 4
        r.record_science_check(301, "10", 0, 6, Some(ActionClass::TakeCard)); // BGO says 6

        assert_eq!(r.science_oracle_checked, 1);
        assert_eq!(r.science_oracle_agreed, 0);
        let d = r.science_oracle_divergence.expect("a real 4 vs 6 mismatch must be flagged");
        assert_eq!(d.lineno, 301);
        assert_eq!(d.round, "10");
        assert_eq!(d.journal_now, 6);
        assert_eq!(d.reconstructed, 4);
        assert_eq!(d.reconstructed - d.journal_now, -2, "this binary under-counts by 2, matching the known 7523162 repro");
    }

    /// The resources oracle's own parity tests -- same shape as science's
    /// pair just above, `resources`/`RESOURCE_ORACLE` in place of
    /// `science`/`SCIENCE_ORACLE`.
    #[test]
    fn flush_pending_resource_check_defers_while_the_actors_own_discard_choice_is_still_open() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let mut options = crate::state::OptionList::new();
        let legion = CardId::by_name("Legion").expect("Legion is a known military card");
        options.push(ChoiceOption::Card(legion));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::DiscardMilitary, options }));
        r.state.players[0].resources = 2; // pre-production, must not be compared yet
        r.pending_resource_check = Some(PendingResourceCheck {
            lineno: 64,
            round: "9".to_string(),
            actor_seat: 0,
            journal_now: 6,
            last_action_class: Some(ActionClass::TakeCard),
        });

        r.flush_pending_resource_check();

        assert_eq!(r.resource_oracle_checked, 0, "must not count a checkpoint whose production has not run yet");
        assert!(r.resource_oracle_divergence.is_none(), "must not flag a false divergence while still blocked");
        assert!(r.pending_resource_check.is_some(), "the check must be put back, not dropped, while still blocked");
    }

    #[test]
    fn record_resource_check_flags_a_real_divergence_with_the_correct_sign() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.last_end_of_turn_resources[0] = Some(9); // this binary's own reconstruction: 9
        r.record_resource_check(240, "7", 0, 4, Some(ActionClass::BuildWonderStage)); // BGO says 4

        assert_eq!(r.resource_oracle_checked, 1);
        assert_eq!(r.resource_oracle_agreed, 0);
        let d = r.resource_oracle_divergence.expect("a real 9 vs 4 mismatch must be flagged");
        assert_eq!(d.reconstructed - d.journal_now, 5, "this binary over-counts by 5");
    }

    /// Real game `7523612` round 14 (`docs/REPLAY.md`'s "Culture oracle"
    /// section, `WinWar`-bucket trace): Purple's own `EndTurn` opens a
    /// `DiscardMilitary` pending exactly as the test above, but resolving it
    /// (`game::resume_end_turn`) does NOT stop at production -- it falls
    /// straight into `advance_turn`, which starts ORANGE's turn and resolves
    /// a war Orange already declared on Purple, moving 15 culture from
    /// Purple to Orange. By the time this checkpoint is ever read,
    /// `state.players[0].culture` (Purple, seat 0 here) has ALREADY been
    /// discounted by that war -- reading it live would report a false -15
    /// divergence for a total that was exactly right the instant production
    /// finished. `record_culture_check` must read the SNAPSHOT `resume_end_
    /// turn` took before `advance_turn` ran, not the live (now war-adjusted)
    /// total.
    #[test]
    fn flush_pending_culture_check_uses_the_pre_advance_turn_snapshot_not_a_later_wars_live_total() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        assert!(r.state.pending.is_empty(), "fixture assumption: the discard already resolved, nothing left open");
        // `resume_end_turn`'s own snapshot: Purple's true post-production
        // total, matching BGO's own "(now 64)" on the very same line.
        r.state.last_end_of_turn_culture[0] = Some(64);
        // The SAME `resume_end_turn` call then ran `advance_turn`, starting
        // Orange's turn and resolving Orange's war -- Purple's LIVE culture
        // is now 49 (64 - 15), the war's real effect, but NOT what this
        // checkpoint (Purple's OWN end-of-turn total) is supposed to read.
        r.state.players[0].culture = 49;
        r.pending_culture_check =
            Some(PendingCultureCheck { lineno: 267, actor_seat: 0, journal_now: 64, last_action_class: Some(ActionClass::WinWar) });

        r.flush_pending_culture_check();

        assert_eq!(r.culture_oracle_checked, 1);
        assert_eq!(r.culture_oracle_agreed, 1, "64 == 64: the snapshot, not the war-adjusted live total, is what must be compared");
        assert!(
            r.culture_oracle_divergence.is_none(),
            "must not report a false -15 divergence for a war that resolved AFTER this checkpoint's true moment"
        );
    }

    /// `parse_game_data_updated_culture`'s own doc, real game `7523809` line
    /// 344: a single admin-correction line can carry more than one player's
    /// clause, and can also carry OTHER stats (science/Bronze in other
    /// sampled games) this function must ignore -- only `culture` clauses
    /// are this oracle's business.
    #[test]
    fn parse_game_data_updated_culture_reads_every_culture_clause_and_ignores_other_stats() {
        assert_eq!(
            parse_game_data_updated_culture("GAME DATA UPDATED Orange culture: 86 -> 80; Purple culture: 65 -> 71"),
            vec![(Color::Orange, 86, 80), (Color::Purple, 65, 71)],
        );
        assert_eq!(
            parse_game_data_updated_culture("GAME DATA UPDATED Green science: 2 -> 5; Green Bronze: 0 -> 2"),
            Vec::new(),
            "science/Bronze corrections are other functions' business (Bronze and science each have their own \
             parser twins below) -- neither is a culture clause, so this must stay empty"
        );
        assert_eq!(parse_game_data_updated_culture("Orange builds Swordsmen Orange spends 3 resources"), Vec::new());
    }

    /// The resources twin of the test just above -- `parse_game_data_
    /// updated_resources` must read every `Bronze` clause and ignore
    /// `culture`/`science`, the mirror-image gap.
    #[test]
    fn parse_game_data_updated_resources_reads_every_bronze_clause_and_ignores_other_stats() {
        assert_eq!(
            parse_game_data_updated_resources("GAME DATA UPDATED Orange Bronze: 2 -> 6"),
            vec![(Color::Orange, 2, 6)],
        );
        assert_eq!(
            parse_game_data_updated_resources("GAME DATA UPDATED Green science: 2 -> 5; Green Bronze: 0 -> 2"),
            vec![(Color::Green, 0, 2)],
        );
        assert_eq!(
            parse_game_data_updated_resources("GAME DATA UPDATED Orange culture: 86 -> 80; Purple culture: 65 -> 71"),
            Vec::new(),
            "culture-only corrections are a different, out-of-scope oracle -- must not be misread as Bronze"
        );
        assert_eq!(parse_game_data_updated_resources("Orange builds Swordsmen Orange spends 3 resources"), Vec::new());
    }

    /// The science twin of the two tests above -- `parse_game_data_updated_
    /// science` must read every `science` clause and ignore `culture`/
    /// `Bronze`, the mirror-image gap. The `7522617` line 105 fixture is
    /// the exact real line this parser exists for (`IllegalMove: Develop`
    /// bucket, `docs/REPLAY.md`).
    #[test]
    fn parse_game_data_updated_science_reads_every_science_clause_and_ignores_other_stats() {
        assert_eq!(
            parse_game_data_updated_science("GAME DATA UPDATED Green science: 7 -> 8; Green Bronze: 4 -> 5"),
            vec![(Color::Green, 7, 8)],
            "the real 7522617 line 105: only the science clause is this parser's business"
        );
        assert_eq!(
            parse_game_data_updated_science("GAME DATA UPDATED Green science: 2 -> 5; Green Bronze: 0 -> 2"),
            vec![(Color::Green, 2, 5)],
        );
        assert_eq!(
            parse_game_data_updated_science("GAME DATA UPDATED Orange culture: 86 -> 80; Purple culture: 65 -> 71"),
            Vec::new(),
            "culture-only corrections are a different, out-of-scope oracle -- must not be misread as science"
        );
        assert_eq!(parse_game_data_updated_science("Orange builds Swordsmen Orange spends 3 resources"), Vec::new());
    }

    /// The actual bug this fixes (`IllegalMove: Develop` bucket, real
    /// game `7521364` line 22-23): BGO's own admin correction ("Orange
    /// Bronze: 2 -> 6") landed one line before Orange's own "builds 1 stage
    /// of Colossus ... spends 3 resources" -- this reconstruction's live
    /// resources (2) exactly matches the correction's stated `old`, so it
    /// must apply immediately, not sit pending, turning the following
    /// WonderStep from illegal (resources=2, cost=3) into legal (resources=6).
    #[test]
    fn flush_pending_resource_corrections_applies_the_bronze_clause_that_unblocks_a_wonder_step() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].resources = 2;
        r.pending_resource_corrections.push((Color::Orange, 2, 6));

        r.flush_pending_resource_corrections();
        assert_eq!(r.state.players[0].resources, 6, "old (2) matches live (2): the correction must apply at once");
        assert!(r.pending_resource_corrections.is_empty(), "consumed, not left pending forever");
    }

    /// TOKENCOUNT regression (2026-08-14): before this fix,
    /// `flush_pending_resource_corrections` set `p.resources = new` directly
    /// -- correcting the SCALAR but never touching `p.resource_tokens` (the
    /// per-denomination placement history, `crate::state::TokenBank`'s own
    /// doc). `economy::blue_used`'s reconciliation only tops the tracked bank
    /// up on an INCREASE; it silently accepts a stale, too-small tracked
    /// bank as-is when the corrected total is smaller than what the bank
    /// still (wrongly) held, permanently inflating the reconstructed token
    /// COUNT relative to the corrected VALUE. This asserts the fixed
    /// `+4` correction (2 -> 6) is reflected in `resource_tokens` too, i.e.
    /// the bank's own tracked value matches `p.resources` afterward, not
    /// just the scalar the previous test already covers. Confirmed RED by
    /// reverting `flush_pending_resource_corrections` to the bare
    /// `p.resources = new.max(0) as u16` it used before this pass: this
    /// assertion fails (tracked value stays 0, `p.resources` becomes 6).
    #[test]
    fn flush_pending_resource_corrections_keeps_the_token_bank_in_sync_with_the_corrected_scalar() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        // Reach resources=2 through the real chokepoint (not a bare `=2`,
        // which per `crate::state::TokenBank`'s own doc is the "never
        // tracked" hand-built-test convention every OTHER test here relies
        // on and would make this specific assertion meaningless -- a fresh
        // player's tracked bank must start genuinely in sync, the way a real
        // replay's always does, for a stale-bank regression to be visible).
        economy::gain_resources(&mut r.state.players[0], 2);
        r.pending_resource_corrections.push((Color::Orange, 2, 6));

        r.flush_pending_resource_corrections();

        let p = &r.state.players[0];
        let tracked: u32 =
            (0..p.resource_tokens.len as usize).map(|i| p.resource_tokens.denoms[i] as u32 * p.resource_tokens.counts[i] as u32).sum();
        assert_eq!(tracked, p.resources as u32, "the tracked bank must account for every corrected resource, not just the scalar");
    }

    /// Same self-deferring shape as `flush_pending_culture_corrections_
    /// defers_until_the_live_value_matches_bgos_own_old_baseline` -- a
    /// `Bronze` correction whose stated `old` does not (yet) match this
    /// reconstruction's live resources must be queued, not guessed at.
    #[test]
    fn flush_pending_resource_corrections_defers_until_the_live_value_matches_bgos_own_old_baseline() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].resources = 9; // live resources not yet at BGO's stated baseline of 4
        r.pending_resource_corrections.push((Color::Orange, 4, 9));

        r.flush_pending_resource_corrections();
        assert_eq!(r.state.players[0].resources, 9, "old (4) does not match live (9) -- must NOT guess and apply anyway");
        assert_eq!(
            r.pending_resource_corrections,
            vec![(Color::Orange, 4, 9)],
            "the correction must be put back, not dropped, while still unmatched"
        );
    }

    /// The actual bug this fixes (`IllegalMove: Develop` bucket, real game
    /// `7522617` line 105-107): BGO's own admin correction ("Green science:
    /// 7 -> 8") landed one line before Green's own "discovers Monarchy ...
    /// loses 8 science" -- this reconstruction's live science (7) exactly
    /// matches the correction's stated `old`, so it must apply immediately,
    /// not sit pending, turning the following Develop from illegal
    /// (science=7, cost=8) into legal (science=8).
    #[test]
    fn flush_pending_science_corrections_applies_the_science_clause_that_unblocks_a_develop() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[Color::Green.seat() as usize].science = 7;
        r.pending_science_corrections.push((Color::Green, 7, 8));

        r.flush_pending_science_corrections();
        assert_eq!(r.state.players[Color::Green.seat() as usize].science, 8, "old (7) matches live (7): the correction must apply at once");
        assert!(r.pending_science_corrections.is_empty(), "consumed, not left pending forever");
    }

    /// Same self-deferring shape as `flush_pending_culture_corrections_
    /// defers_until_the_live_value_matches_bgos_own_old_baseline` -- a
    /// `science` correction whose stated `old` does not (yet) match this
    /// reconstruction's live science must be queued, not guessed at.
    #[test]
    fn flush_pending_science_corrections_defers_until_the_live_value_matches_bgos_own_old_baseline() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[Color::Green.seat() as usize].science = 5; // live science not yet at BGO's stated baseline of 7
        r.pending_science_corrections.push((Color::Green, 7, 8));

        r.flush_pending_science_corrections();
        assert_eq!(r.state.players[Color::Green.seat() as usize].science, 5, "old (7) does not match live (5) -- must NOT guess and apply anyway");
        assert_eq!(
            r.pending_science_corrections,
            vec![(Color::Green, 7, 8)],
            "the correction must be put back, not dropped, while still unmatched"
        );
    }

    /// The actual bug this fixes (`docs/REPLAY.md`'s "Culture oracle"
    /// section, game `7523809` line 344): BGO's own admin correction
    /// ("Orange culture: 86 -> 80") can land on a journal line BEFORE this
    /// reconstruction's own Orange culture has caught up to 86 at all --
    /// Orange's War-over-Culture win (+22) is itself deferred on our side
    /// until `game::start_turn`'s cascade, which has not run yet the
    /// instant this line is read (live culture is still 64). Applying the
    /// -6 delta THEN would land on the wrong baseline (64, not 86) and
    /// silently corrupt state. `flush_pending_culture_corrections` must
    /// defer, not guess, the same self-deferring shape as `flush_pending_
    /// culture_check` above.
    #[test]
    fn flush_pending_culture_corrections_defers_until_the_live_value_matches_bgos_own_old_baseline() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].culture = 64; // Orange's live total: the war win has not cascaded in yet
        r.pending_culture_corrections.push((Color::Orange, 86, 80));

        r.flush_pending_culture_corrections();
        assert_eq!(r.state.players[0].culture, 64, "old (86) does not match live (64) yet -- must NOT guess and apply anyway");
        assert_eq!(
            r.pending_culture_corrections,
            vec![(Color::Orange, 86, 80)],
            "the correction must be put back, not dropped, while still unmatched"
        );

        // A few journal lines later, `game::start_turn`'s cascade has now
        // run and Orange's live culture has caught up to BGO's own stated
        // baseline (64 + 22 from the war win = 86) -- the SAME deferred
        // correction now applies correctly.
        r.state.players[0].culture = 86;
        r.flush_pending_culture_corrections();
        assert_eq!(r.state.players[0].culture, 80, "86 == 86: now safe to apply BGO's own -6 correction");
        assert!(r.pending_culture_corrections.is_empty(), "consumed, not left pending forever");
    }

    #[test]
    fn top_up_civil_deck_is_a_no_op_once_already_at_the_floor() {
        let mut state = game::new_game(2, 1);
        state.civil_deck = CardList::new();
        for _ in 0..CIVIL_DECK_SAFETY_FLOOR {
            state.civil_deck.push(CardId::by_name("Bronze").unwrap());
        }
        let before = state.civil_deck.as_slice().to_vec();
        top_up_civil_deck(&mut state);
        assert_eq!(state.civil_deck.as_slice(), before.as_slice());
    }

    /// The actual bug this function exists to prevent (`docs/REPLAY.md`'s
    /// "civil deck model" handoff, game `7523449`): `game::deal`'s own
    /// embedded `advance_age` trigger fires the moment `civil_deck.pop()`
    /// empties it. A deck below the floor must be topped back up to it.
    #[test]
    fn top_up_civil_deck_refills_a_low_deck_up_to_the_floor() {
        let mut state = game::new_game(2, 1);
        state.age_civil = crate::cards::Age::I;
        state.civil_deck = CardList::new();
        state.civil_deck.push(CardId::by_name("Bronze").unwrap());
        assert!(state.civil_deck.len() < CIVIL_DECK_SAFETY_FLOOR);

        top_up_civil_deck(&mut state);

        assert!(
            state.civil_deck.len() >= CIVIL_DECK_SAFETY_FLOOR,
            "left at {} cards, floor is {CIVIL_DECK_SAFETY_FLOOR}",
            state.civil_deck.len()
        );
    }

    /// §12.3: Age IV has no civil deck at all (`game::advance_age`'s own
    /// `nxt == Age::IV` branch empties it outright and never refills it) --
    /// `top_up_civil_deck` must leave that alone rather than manufacture a
    /// deck for an age that structurally has none.
    #[test]
    fn top_up_civil_deck_does_not_touch_age_iv_which_has_no_civil_deck_at_all() {
        let mut state = game::new_game(2, 1);
        state.age_civil = crate::cards::Age::IV;
        state.civil_deck = CardList::new();
        top_up_civil_deck(&mut state);
        assert!(state.civil_deck.is_empty());
    }

    /// Every real age's own civil-card pool is comfortably larger than the
    /// floor at every player count -- pinned directly (not just relied on)
    /// so a future change to either the floor or the card data fails here
    /// first, with a clear message, instead of surfacing as `top_up_civil_
    /// deck`'s silent "reserve ran out early" `break`.
    #[test]
    fn top_up_civil_deck_reserve_batch_is_never_smaller_than_the_floor() {
        for n in 2..=4usize {
            for age in [crate::cards::Age::I, crate::cards::Age::II, crate::cards::Age::III] {
                let deck = game::build_deck(age, true, n);
                assert!(
                    deck.len() >= CIVIL_DECK_SAFETY_FLOOR,
                    "{age:?} civil deck at {n}p is only {} cards, floor is {CIVIL_DECK_SAFETY_FLOOR}",
                    deck.len()
                );
            }
        }
    }

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
    fn civil_and_military_uses_keeps_the_two_clauses_separate_unlike_total_action_cost() {
        // Same real line `total_action_cost_sums_a_civil_and_a_military_
        // clause_on_one_line` above reads as a combined 2 -- the
        // civil-action-TOTAL undercount check (docs/REPLAY.md "civil
        // action total" handoff) needs the two kept apart instead, since
        // this is Hammurabi's once-per-turn MA-for-CA conversion paying
        // the printed civil price out of the military pool, not a take
        // costing 2 combined action points.
        let text = "Orange takes Breakthrough in hand Orange uses 1 civil action; \
                     Orange uses 1 military action";
        assert_eq!(civil_and_military_uses(text), (Some(1), Some(1)));
    }

    #[test]
    fn civil_and_military_uses_reads_a_civil_only_line_with_no_military_clause() {
        let text = "Orange takes Engineering Genius in hand Orange uses 1 civil action";
        assert_eq!(civil_and_military_uses(text), (Some(1), None));
    }

    #[test]
    fn civil_and_military_uses_is_none_none_with_no_uses_clause_at_all() {
        assert_eq!(civil_and_military_uses("Orange increases population Orange spends 2 food"), (None, None));
    }

    #[test]
    fn trailing_gets_civil_action_reads_a_leader_replacement_refund() {
        // Real corpus line, game `7522895`: replacing a leader refunds the
        // civil action spent playing the old one (RB p.11, CoL p.5 --
        // `apply.rs`'s own "Replacing a leader refunds one civil action").
        let text = "Orange elects Michelangelo Hammurabi dies; Orange gets 1 civil action";
        assert_eq!(trailing_gets_civil_action(text), Some(1));
    }

    #[test]
    fn trailing_gets_civil_action_reads_a_putback_refund() {
        let text = "Grey puts Frugality back in the row Grey gets 2 civil action";
        assert_eq!(trailing_gets_civil_action(text), Some(2));
    }

    #[test]
    fn trailing_gets_civil_action_is_none_when_no_such_clause_is_present() {
        assert_eq!(trailing_gets_civil_action("Orange elects Hammurabi"), None);
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

    /// Real corpus shapes (game `7523338` line 174 and others,
    /// `docs/REPLAY.md`): a resources-only split, a food-only split, and a
    /// mixed split, singular vs plural handled correctly ("1 resource" has
    /// no trailing "s" to trip over).
    #[test]
    fn parse_plunder_split_line_reads_every_real_corpus_split_shape() {
        assert_eq!(
            parse_plunder_split_line("Purple produces 3 resources; Green spends 3 resources"),
            Some((Color::Purple, 0, 3))
        );
        assert_eq!(
            parse_plunder_split_line("Purple produces 3 food; Green spends 3 food"),
            Some((Color::Purple, 3, 0))
        );
        assert_eq!(
            parse_plunder_split_line(
                "Orange produces 5 food; Orange produces 2 resources; Purple spends 5 food; Purple spends 2 resources"
            ),
            Some((Color::Orange, 5, 2))
        );
        assert_eq!(
            parse_plunder_split_line(
                "Grey produces 4 food; Grey produces 1 resource; Purple spends 4 food; Purple spends 1 resource"
            ),
            Some((Color::Grey, 4, 1))
        );
    }

    /// Foray/Refugees' deterministic "and/or" grant (`events::food_or_
    /// resources`) prints the SAME "<Color> produces X food; <Color>
    /// produces Y resources" shape with no victim -- must NOT be mistaken
    /// for a Plunder resolution (real corpus line, game `7521158`).
    #[test]
    fn parse_plunder_split_line_rejects_a_forays_deterministic_grant_with_no_victim_clause() {
        assert_eq!(parse_plunder_split_line("Green produces 1 food; Green produces 2 resources"), None);
    }

    #[test]
    fn parse_plunder_split_line_rejects_an_unrelated_produces_line() {
        assert_eq!(parse_plunder_split_line("Purple builds Bronze Purple spends 2 resources"), None);
    }

    /// [`parse_produces_grant_line`]: the mirror image of the two tests
    /// above -- must read Foray/Refugees' own "and/or" grant shape (real
    /// corpus line, game `7523357` round 8), in every clause order/count,
    /// and must REJECT a real Plunder resolution (the trailing victim
    /// `"spends"` clause is exactly what distinguishes the two, same
    /// signature `parse_plunder_split_line` checks for, inverted).
    #[test]
    fn parse_produces_grant_line_reads_a_forays_own_split_but_rejects_a_plunder_resolution() {
        assert_eq!(parse_produces_grant_line("Grey produces 2 food; Grey produces 1 resource"), Some((Color::Grey, 2, 1)));
        assert_eq!(parse_produces_grant_line("Green produces 1 food; Green produces 2 resources"), Some((Color::Green, 1, 2)));
        assert_eq!(parse_produces_grant_line("Purple produces 3 resources"), Some((Color::Purple, 0, 3)));
        assert_eq!(parse_produces_grant_line("Orange produces 2 food"), Some((Color::Orange, 2, 0)));
        // A real Plunder resolution (trailing victim "spends" clause) must
        // be left to `parse_plunder_split_line`, not double-matched here.
        assert_eq!(
            parse_produces_grant_line("Purple produces 3 resources; Green spends 3 resources"),
            None
        );
        assert_eq!(parse_produces_grant_line("Purple builds Bronze Purple spends 2 resources"), None);
    }

    /// TOKENCOUNT regression (2026-08-14): [`apply_journal_food_or_res_correction`]
    /// must roll a wrong DEFAULT guess (all 3 into resources, mismatching
    /// the journal's real "1 food; 2 resources" Foray split) back out of
    /// BOTH the scalar and the tracked bank, then place the journal's true
    /// split through the real chokepoints -- not just fix the scalar and
    /// leave the bank holding the wrong guess's tokens. Player has a single
    /// denom-1 Farm/Mine (`Denoms::of` always includes `1`), so the guessed
    /// `gain_resources(3)` would have placed 3 denom-1 tokens; after the
    /// correction the bank must hold exactly 1 food token (value 1) and 2
    /// resource tokens (value 2), matching the corrected scalars, not the
    /// 3-resource-token guess. Confirmed RED by reverting the fix (restoring
    /// the bare `p.food = pre_food + jf as i32` / `p.resources = pre_res +
    /// jr as i32` this replaced): `resource_tokens` still shows 3 tracked
    /// tokens (the unrolled guess) while `p.resources` reads 2, and
    /// `food_tokens` shows 0 tracked tokens while `p.food` reads 1.
    #[test]
    fn apply_journal_food_or_res_correction_rolls_back_the_wrong_guess_and_resyncs_the_bank() {
        let mut state = crate::game::new_game(2, 0xC0FFEE);
        let p = &mut state.players[0];
        let pre_food = p.food;
        let pre_res = p.resources;
        let pre_food_tokens = p.food_tokens;
        let pre_resource_tokens = p.resource_tokens;
        // The DEFAULT ("resources first") guess: all 3 into resources.
        economy::gain_resources(p, 3);
        assert_eq!(p.resources, pre_res + 3, "fixture assumption: the guess applied cleanly");

        // The journal's own true split: 1 food, 2 resources.
        apply_journal_food_or_res_correction(p, pre_food, pre_res, pre_food_tokens, pre_resource_tokens, 1, 2, false);

        assert_eq!(p.food, pre_food + 1, "the journal's true food share must land");
        assert_eq!(p.resources, pre_res + 2, "the journal's true resources share must land, not the guessed 3");
        let food_tracked: u32 = (0..p.food_tokens.len as usize).map(|i| p.food_tokens.denoms[i] as u32 * p.food_tokens.counts[i] as u32).sum();
        let res_tracked: u32 =
            (0..p.resource_tokens.len as usize).map(|i| p.resource_tokens.denoms[i] as u32 * p.resource_tokens.counts[i] as u32).sum();
        assert_eq!(food_tracked, p.food as u32, "food_tokens must account for the corrected food, not the guess's zero");
        assert_eq!(res_tracked, p.resources as u32, "resource_tokens must account for the corrected 2, not the guessed 3");
    }

    /// Real corpus shapes for BGO's own §6.6-step-1 announcement, singular
    /// and plural, `"No Discard Phase"`, and the largest real value observed
    /// corpus-wide (`14`, game-independent -- just the parser's own upper
    /// range) -- see the "Discard-phase hand-size oracle" module doc.
    #[test]
    fn parse_discard_phase_announcement_reads_every_real_corpus_shape() {
        assert_eq!(parse_discard_phase_announcement("No Discard Phase"), Some(0));
        assert_eq!(parse_discard_phase_announcement("Discard Phase 1 military card must be discarded"), Some(1));
        assert_eq!(parse_discard_phase_announcement("Discard Phase 2 military cards must be discarded"), Some(2));
        assert_eq!(parse_discard_phase_announcement("Discard Phase 14 military cards must be discarded"), Some(14));
    }

    #[test]
    fn parse_discard_phase_announcement_rejects_unrelated_text() {
        assert_eq!(parse_discard_phase_announcement("Action Phase begins"), None);
        assert_eq!(parse_discard_phase_announcement("Orange discards 1 card"), None);
        // Malformed/truncated shapes must not be mistaken for a real one.
        assert_eq!(parse_discard_phase_announcement("Discard Phase military card must be discarded"), None);
        assert_eq!(parse_discard_phase_announcement("Discard Phase 2 military cards must be"), None);
    }

    /// The `"<Color> discards N card(s)"` resolution line -- singular/plural,
    /// and rejecting a line that merely starts the same way (`"discards"` is
    /// also the verb for civil-hand-limit debug text elsewhere, so the
    /// trailing shape has to be checked exactly, not just the prefix).
    #[test]
    fn parse_discard_count_line_reads_singular_and_plural() {
        assert_eq!(parse_discard_count_line("Orange discards 1 card"), Some((Color::Orange, 1)));
        assert_eq!(parse_discard_count_line("Purple discards 4 cards"), Some((Color::Purple, 4)));
    }

    #[test]
    fn parse_discard_count_line_rejects_unrelated_text() {
        assert_eq!(parse_discard_count_line("No Discard Phase"), None);
        assert_eq!(parse_discard_count_line("Orange discards Legion"), None);
        assert_eq!(parse_discard_count_line("Orange passes Political Phase"), None);
    }

    /// Builds a bare `Line` for [`prescan_discard_phase_oracle`]'s own
    /// tests -- only the four fields that function reads matter to it.
    fn oracle_test_line<'a>(color: &'a str, round: &'a str, text: &'a str) -> Line<'a> {
        Line { lineno: 0, color, age: "I", round, text }
    }

    /// The ordinary, overwhelmingly common case: the announcement and the
    /// resolution line agree, so the entry is trusted and carries the
    /// announced count through.
    #[test]
    fn discard_phase_oracle_trusts_an_entry_where_both_journal_renderings_agree() {
        let lines = [
            oracle_test_line("Orange", "4", "Discard Phase 1 military card must be discarded"),
            oracle_test_line("Orange", "4", "End turn Orange scores:; ; 1 culture (now 1)"),
            oracle_test_line("Orange", "4", "Orange discards 1 card"),
        ];
        let oracle = prescan_discard_phase_oracle(&lines);
        assert_eq!(oracle.get(&(Color::Orange.seat(), "4".to_string())), Some(&1));
    }

    /// `"No Discard Phase"` with no matching resolution line at all is its
    /// own agreement case (announced 0, actual 0 by absence) -- must be
    /// trusted, not dropped for "missing" a resolution line.
    #[test]
    fn discard_phase_oracle_trusts_a_no_discard_phase_entry_with_no_resolution_line() {
        let lines = [oracle_test_line("Purple", "7", "No Discard Phase")];
        let oracle = prescan_discard_phase_oracle(&lines);
        assert_eq!(oracle.get(&(Color::Purple.seat(), "7".to_string())), Some(&0));
    }

    /// The real, documented failure mode this gate exists for: BGO's two
    /// independent renderings of the same fact disagree (the announcement
    /// says 1, the later resolution line says 2 -- the exact shape found on
    /// real game `7521072` round 6). Neither is trusted; the entry must be
    /// ABSENT from the oracle, not silently resolved to one side or the
    /// other, so [`Replayer::check_discard_phase_oracle`] skips it rather
    /// than reporting a false divergence caused by a journal
    /// self-inconsistency instead of a real reconstruction bug.
    #[test]
    fn discard_phase_oracle_drops_an_entry_where_the_two_journal_renderings_disagree() {
        let lines = [
            oracle_test_line("Orange", "6", "Discard Phase 1 military card must be discarded"),
            oracle_test_line("Orange", "6", "End turn Orange scores:; ; 0 culture (now 4)"),
            oracle_test_line("Orange", "6", "Orange discards 2 cards"),
        ];
        let oracle = prescan_discard_phase_oracle(&lines);
        assert_eq!(oracle.get(&(Color::Orange.seat(), "6".to_string())), None);
    }

    // -----------------------------------------------------------------
    // Military hand ledger
    // -----------------------------------------------------------------

    #[test]
    fn parse_military_draw_clause_reads_singular_and_plural() {
        assert_eq!(parse_military_draw_clause("Orange draws 1 military card"), Some((Color::Orange, 1)));
        assert_eq!(parse_military_draw_clause("Purple draws 3 military cards"), Some((Color::Purple, 3)));
    }

    /// The whole-line description clause a "Development of Politics"/
    /// "Politics of Strength" reveal glues on ("Each player draws 3 military
    /// cards.") must NOT be mistaken for a real per-player draw -- it has no
    /// leading colour at all, unlike the SEPARATE `"<Color> draws N..."`
    /// clauses the same line also carries (one per recipient), which DO
    /// match. Confirms the caller's own clause-splitting is what makes this
    /// distinction, not this parser alone.
    #[test]
    fn parse_military_draw_clause_rejects_the_no_actor_description_clause() {
        assert_eq!(parse_military_draw_clause("Each player draws 3 military cards."), None);
        assert_eq!(parse_military_draw_clause("Orange discards 1 card"), None);
    }

    #[test]
    fn defense_consumed_count_reads_a_bonus_card_clause() {
        assert_eq!(
            defense_consumed_count("Orange defends 1 Defense card +6 played; Orange strength: 26; Purple strength: 26"),
            Some((Color::Orange, 1))
        );
    }

    #[test]
    fn defense_consumed_count_reads_multiple_flat_clauses_on_a_tries_to_defend_line() {
        // Real corpus shape (`parse_defense_clauses` -- the function this
        // ledger deliberately does NOT share -- only recognises the
        // "defends " prefix; "tries to defend" is BGO's OTHER phrasing, used
        // when the defender's own committed force still loses).
        let text = "Purple tries to defend; 1 military card played; 1 military card played; 1 military card played; \
                     Purple strength: 18; Orange strength: 25";
        assert_eq!(defense_consumed_count(text), Some((Color::Purple, 3)));
    }

    #[test]
    fn defense_consumed_count_is_zero_for_a_defense_with_nothing_committed() {
        assert_eq!(
            defense_consumed_count("Orange defends Orange strength: 9; Purple strength: 9"),
            Some((Color::Orange, 0))
        );
        assert_eq!(
            defense_consumed_count("Purple tries to defend; Purple strength: 9; Orange strength: 15"),
            Some((Color::Purple, 0))
        );
    }

    #[test]
    fn defense_consumed_count_rejects_an_unrelated_line() {
        assert_eq!(defense_consumed_count("Orange discards 1 card"), None);
    }

    /// Real corpus shape, game `7522614` round 2-4 (`docs/REPLAY.md`'s
    /// "Card-by-card audit" section): two ordinary end-of-turn draws (2 then
    /// 2 more) with nothing else happening land the ledger on exactly 4 at
    /// the round-4 checkpoint -- and, critically, round 2's OWN draw (glued
    /// onto round 2's own "End turn" line) must NOT count toward round 2's
    /// OWN checkpoint (recorded strictly before that line's clauses are
    /// applied), landing in round 3's checkpoint instead. This is the
    /// checkpoint-timing property the whole ledger depends on.
    #[test]
    fn military_hand_ledger_excludes_a_rounds_own_draw_from_its_own_checkpoint() {
        let lines = [
            oracle_test_line("Orange", "2", "No Discard Phase"),
            oracle_test_line(
                "Orange",
                "2",
                "End turn Orange scores:; ; 0 culture (now 0); 1 science (now 2); 3 food - consumption: 0 (now 5); \
                 2 resources (now 2); Orange draws 2 military cards",
            ),
            oracle_test_line("Orange", "3", "No Discard Phase"),
            oracle_test_line(
                "Orange",
                "3",
                "End turn Orange scores:; ; 0 culture (now 0); 1 science (now 2); 3 food - consumption: 0 (now 6); \
                 2 resources (now 2); Orange draws 2 military cards",
            ),
            oracle_test_line("Orange", "4", "Discard Phase 1 military card must be discarded"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        let seat = Color::Orange.seat();
        assert_eq!(ledger.get(&(seat, "2".to_string())).map(|c| c.raw), Some(0));
        assert_eq!(ledger.get(&(seat, "3".to_string())).map(|c| c.raw), Some(2));
    }

    /// §5.2 `PrepareEvent` pulls a card OUT of hand -- the ledger must count
    /// it as a real -1, not the net-zero wash `resolve_political_decision`'s
    /// own push-then-apply-removal sequence used to produce before its own
    /// fix (`docs/REPLAY.md`'s "PrepareEvent's net-zero push" section). Also
    /// covers the SAME line granting a draw to the SAME actor as one of an
    /// event's "each player draws" recipients (Development of Politics),
    /// confirming the two effects net additively on one line rather than one
    /// clobbering the other.
    #[test]
    fn military_hand_ledger_counts_a_prepare_event_as_minus_one_even_when_the_same_line_also_draws() {
        let lines = [
            oracle_test_line(
                "Green",
                "4",
                "Green plays event Green scores 1 culture; Current event:; A / Development of Politics; \
                 Each player draws 3 military cards.; Orange draws 3 military cards; Green draws 3 military cards",
            ),
            oracle_test_line("Green", "5", "End turn Green scores:; ; 0 culture (now 1)"),
            oracle_test_line("Orange", "5", "End turn Orange scores:; ; 0 culture (now 0)"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        // Green: -1 (prepares) + 3 (its own "each player" draw) = net +2.
        assert_eq!(ledger.get(&(Color::Green.seat(), "5".to_string())).map(|c| c.raw), Some(2));
        // Orange only drew (from the SAME line, as another "each player"
        // recipient), no preparation of its own -- confirms the -1 above is
        // scoped to the preparing actor, not applied to every drawer.
        assert_eq!(ledger.get(&(Color::Orange.seat(), "5".to_string())).map(|c| c.raw), Some(3));
    }

    /// A named play (`DeclareWar`/`PlayAggression`/`ProposePact`/
    /// `PlayTactic` excluding `CopyTactic`) consumes one hand card by
    /// identity -- same predicate `prescan_future_military_needs` uses for
    /// `DiscardSolver`, reused here as a ledger event.
    #[test]
    fn military_hand_ledger_counts_a_named_war_declaration_as_minus_one() {
        let lines = [
            oracle_test_line(
                "Orange",
                "6",
                "End turn Orange scores:; ; 0 culture (now 0); 1 science (now 2); 2 food - consumption: 0 (now 5); \
                 2 resources (now 2); Orange draws 2 military cards",
            ),
            oracle_test_line("Orange", "6", "No Discard Phase"),
            oracle_test_line("Orange", "7", "Orange declares War over Culture on Purple The victor takes 5 culture"),
            oracle_test_line("Orange", "7", "End turn Orange scores:; ; 0 culture (now 0)"),
            oracle_test_line("Orange", "8", "End turn Orange scores:; ; 0 culture (now 0)"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        assert_eq!(ledger.get(&(Color::Orange.seat(), "8".to_string())).map(|c| c.raw), Some(1));
    }

    /// FOUND chasing the `UnmodelledEvent`/`PrepareEvent` ledger bucket
    /// (`docs/REPLAY.md`): `"Christopher Columbus discovers <Age> /
    /// <Territory>"` has NO leading actor colour at all (the actor is only
    /// in `Line::color`, exactly like `"End turn"`), so it never reached the
    /// generic `actor_and_rest`-gated dispatch and was silently counted as
    /// ZERO by the ledger even though `apply.rs::h_columbus_colonize`
    /// genuinely removes the discovered territory from `hand_military`
    /// (§`ColumbusColonize`'s own doc: "without sacrificing any units" is
    /// not "without leaving the hand"). Real corpus shape, game `7523353`
    /// line 167: this ledger gap alone left the ledger's own running count
    /// permanently one card too high for the rest of that game, first
    /// surfacing as a divergence 7 rounds later, at a checkpoint whose OWN
    /// `last_event` was an entirely innocent `PrepareEvent`.
    #[test]
    fn military_hand_ledger_counts_a_columbus_discovery_as_minus_one_despite_no_leading_actor_colour() {
        let lines = [
            oracle_test_line(
                "Purple",
                "10",
                "End turn Purple scores:; ; 0 culture (now 0); 1 science (now 2); 2 food - consumption: 0 (now 5); \
                 2 resources (now 2); Purple draws 2 military cards",
            ),
            oracle_test_line("Purple", "11", "Christopher Columbus discovers I / Vast Territory"),
            oracle_test_line("Purple", "11", "End turn Purple scores:; ; 0 culture (now 0)"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        let purple = ledger.get(&(Color::Purple.seat(), "11".to_string())).unwrap();
        assert_eq!(purple.raw, 1, "2 drawn - 1 Columbus consumption = 1, not the silently-uncounted 2");
        assert_eq!(purple.last_event.map(|(kind, _)| kind), Some(LedgerEventKind::ColumbusConsume));
    }

    /// Real corpus shape, Politics of Strength's "weakest civilization
    /// discards N" resolution: `parse_discard_count_line` is reused
    /// per-CLAUSE, so a discard resolution that is NOT part of the ordinary
    /// discard-phase modal (glued onto an unrelated line) is still counted.
    #[test]
    fn military_hand_ledger_counts_a_standalone_discard_line_not_just_the_modal_shape() {
        let lines = [
            oracle_test_line(
                "Orange",
                "5",
                "End turn Orange scores:; ; 0 culture (now 0); 1 science (now 2); 2 food - consumption: 0 (now 5); \
                 2 resources (now 2); Orange draws 3 military cards",
            ),
            oracle_test_line("Green", "5", "Orange discards 3 cards"),
            oracle_test_line("Orange", "6", "End turn Orange scores:; ; 0 culture (now 0)"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        assert_eq!(ledger.get(&(Color::Orange.seat(), "6".to_string())).map(|c| c.raw), Some(0));
    }

    /// REGRESSION (real corpus shape, game `7523347` round 4): BGO does NOT
    /// always print a round's own discard resolution AFTER that round's own
    /// "End turn" line -- here it comes BEFORE (`"Discard Phase..."` ->
    /// `"Green discards 2 cards"` -> `"End turn Green scores: ..."`), the
    /// reverse of the far more common order the sibling test above covers.
    /// A discard belonging to THIS SAME round must still land AFTER this
    /// round's own checkpoint (deferred, not applied immediately), or the
    /// checkpoint would wrongly see itself already short by the very
    /// discard it is supposed to be checked against.
    #[test]
    fn military_hand_ledger_defers_a_same_round_discard_that_prints_before_its_own_end_turn_line() {
        let lines = [
            oracle_test_line("Green", "4", "Discard Phase 2 military cards must be discarded"),
            oracle_test_line("Green", "4", "Green discards 2 cards"),
            oracle_test_line("Green", "4", "End turn Green scores:; ; 0 culture (now 6)"),
            oracle_test_line("Green", "5", "End turn Green scores:; ; 0 culture (now 6); Green draws 2 military cards"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        let seat = Color::Green.seat();
        // Nothing drew into Green's hand before round 4's own checkpoint in
        // this fixture, so round 4's own -2 discard must NOT show up yet --
        // applying it early would read as an impossible negative hand.
        assert_eq!(ledger.get(&(seat, "4".to_string())).map(|c| c.raw), Some(0));
        // The deferred -2 must still land, one round later, once round 4's
        // own checkpoint has been recorded.
        assert_eq!(ledger.get(&(seat, "5".to_string())).map(|c| c.raw), Some(-2));
    }

    /// `last_event` names the most recent ledger-tracked mechanism, the
    /// signal [`HandLedgerVerdict::UnmodelledEvent`] reports -- confirms it
    /// tracks the RIGHT actor's own most recent event, not a global one.
    #[test]
    fn military_hand_ledger_last_event_is_scoped_per_actor() {
        let lines = [
            oracle_test_line(
                "Orange",
                "5",
                "End turn Orange scores:; ; 0 culture (now 0); 1 science (now 2); 2 food - consumption: 0 (now 5); \
                 2 resources (now 2); Orange draws 2 military cards",
            ),
            oracle_test_line(
                "Purple",
                "5",
                "Purple defends 1 Defense card +6 played; Purple strength: 10; Orange strength: 10",
            ),
            oracle_test_line("Orange", "6", "End turn Orange scores:; ; 0 culture (now 0)"),
        ];
        let card_index = build_card_index();
        let ledger = prescan_military_hand_ledger(&lines, &card_index);
        let orange = ledger.get(&(Color::Orange.seat(), "6".to_string())).unwrap();
        assert_eq!(orange.last_event.map(|(kind, _)| kind), Some(LedgerEventKind::Draw));
    }

    /// REGRESSION (chasing the `IllegalMove: Pop` bucket, game `7522658`
    /// line 289): a live Trade Routes Agreement grant lets a Pop be paid
    /// PART in converted resources, and BGO logs that as a SECOND `"spends
    /// M resource"` clause on the SAME line as the food clause -- an
    /// earlier version of `spent_food`'s own doc comment claimed Pop "has
    /// no resource component" and was simply wrong, confirmed against
    /// thousands of real corpus lines with exactly this shape.
    /// `spent_resource_after_food` must read that second clause, not the
    /// food clause's own number, and must return `0` (not panic or find the
    /// food number again) when there is no second clause at all.
    #[test]
    fn spent_resource_after_food_reads_the_second_clause_not_the_first() {
        assert_eq!(spent_resource_after_food("Purple increases population Purple spends 2 food; Purple spends 1 resource"), 1);
        assert_eq!(spent_resource_after_food("Green increases population Green spends 3 food; Green spends 1 resource"), 1);
        // An ordinary Pop with no conversion -- only one "spends" clause.
        assert_eq!(spent_resource_after_food("Grey increases population Grey spends 3 food"), 0);
        // A line with no "spends" clause at all.
        assert_eq!(spent_resource_after_food("Orange passes Political Phase"), 0);
    }

    /// Real corpus shapes for Infiltrate's resolution (`docs/REPLAY.md`'s
    /// six-pending-kind pass, sixth kind): a leader removed on the victim's
    /// own combined `"concedes defeat"` line, a wonder removed the same way,
    /// and the split two-line shape (a bare `"concedes defeat"` from the
    /// victim with nothing to parse, immediately followed by the attacker's
    /// own `"Operation successful"` line carrying the real consequence) --
    /// both prefixes must read identically since the information lives in
    /// the SAME trailing clause shape either way.
    #[test]
    fn parse_infiltrate_line_reads_every_real_corpus_resolution_shape() {
        assert_eq!(
            parse_infiltrate_line("concedes defeat Charles Chaplin is killed; Purple scores 9 culture"),
            Some((Color::Purple, false))
        );
        assert_eq!(
            parse_infiltrate_line("concedes defeat Eiffel Tower is destroyed; Purple scores 6 culture"),
            Some((Color::Purple, true))
        );
        assert_eq!(
            parse_infiltrate_line("Operation successful Universitas Carolina is destroyed; Orange scores 3 culture"),
            Some((Color::Orange, true))
        );
        assert_eq!(
            parse_infiltrate_line("Operation successful Isaac Newton is killed; Purple scores 6 culture"),
            Some((Color::Purple, false))
        );
    }

    /// The bare, clause-less half of the split two-line shape (see the test
    /// above) must parse to `None` -- there is genuinely nothing to read on
    /// this line, the real evidence is the FOLLOWING `"Operation
    /// successful"` line, which the per-line prescan picks up on its own.
    #[test]
    fn parse_infiltrate_line_reads_nothing_off_a_bare_concedes_defeat_line() {
        assert_eq!(parse_infiltrate_line("concedes defeat"), None);
    }

    /// Other Aggression subtypes' own `"concedes defeat"` resolutions (War
    /// over Culture/Science/Territory, Annex) must NOT be mistaken for an
    /// Infiltrate resolution -- none of them contain "is killed"/"is
    /// destroyed" (real corpus lines).
    #[test]
    fn parse_infiltrate_line_rejects_other_aggression_types_own_concedes_defeat_lines() {
        assert_eq!(parse_infiltrate_line("concedes defeat Orange scores 7 culture; Purple loses 7 culture"), None);
        assert_eq!(parse_infiltrate_line("concedes defeat Orange gets 5 science; Purple loses 5 science"), None);
        assert_eq!(parse_infiltrate_line("concedes defeat Green takes II / Vast Territory from Grey"), None);
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
    fn trailing_gets_military_resource_reads_patriotisms_own_bonus_clause_not_the_later_military_action_one() {
        // Real corpus line (game `7521776`, round 6): a naive `rfind(" gets
        // ")` (`trailing_gets_science`'s own approach) would land on "1
        // military action" instead, since that clause comes LAST.
        let text = "plays Patriotism Orange gets 1 military resource; Orange gets 1 military action";
        assert_eq!(trailing_gets_military_resource(text), Some(1));
    }

    #[test]
    fn trailing_gets_military_resource_reads_a_double_digit_amount() {
        let text = "plays Patriotism Purple gets 10 military resource; Purple gets 1 military action";
        assert_eq!(trailing_gets_military_resource(text), Some(10));
    }

    #[test]
    fn trailing_gets_military_resource_is_none_with_no_such_clause() {
        assert_eq!(trailing_gets_military_resource("Orange elects Hammurabi Orange gets 1 civil action"), None);
    }

    #[test]
    fn trailing_up_to_resource_and_or_food_reads_plunders_own_printed_cap() {
        // Real corpus line shape (game `7523357`, line 503): Age III's own
        // printed cap is 7, not the Age I default `build_card_index` would
        // otherwise arbitrarily resolve "Plunder" to.
        let text = "Purple plays Plunder against Grey Your rival loses a total of up to 7 resource \
                     and/or food (your choice). You gain that many resource and food.; ; Purple uses \
                     1 military action";
        assert_eq!(trailing_up_to_resource_and_or_food(text), Some(7));
    }

    #[test]
    fn trailing_up_to_resource_and_or_food_reads_every_real_printed_amount() {
        for (n, text) in [
            (3, "plays Plunder against Grey Your rival loses a total of up to 3 resource and/or food (your choice)."),
            (5, "plays Plunder against Grey Your rival loses a total of up to 5 resource and/or food (your choice)."),
            (7, "plays Plunder against Grey Your rival loses a total of up to 7 resource and/or food (your choice)."),
        ] {
            assert_eq!(trailing_up_to_resource_and_or_food(text), Some(n));
        }
    }

    #[test]
    fn trailing_up_to_resource_and_or_food_is_none_with_no_such_clause() {
        assert_eq!(trailing_up_to_resource_and_or_food("plays Aggression: Raid against Grey"), None);
    }

    #[test]
    fn plunder_food_and_or_resources_reads_each_age_siblings_own_printed_number() {
        assert_eq!(plunder_food_and_or_resources(CardId::by_name("Aggression: Plunder (I)").unwrap()), Some(3));
        assert_eq!(plunder_food_and_or_resources(CardId::by_name("Aggression: Plunder (II)").unwrap()), Some(5));
        assert_eq!(plunder_food_and_or_resources(CardId::by_name("Aggression: Plunder (III)").unwrap()), Some(7));
    }

    #[test]
    fn plunder_food_and_or_resources_is_none_for_a_card_with_no_takefromopponent_block() {
        assert_eq!(plunder_food_and_or_resources(CardId::by_name("Aggression: Raid (I)").unwrap()), None);
    }

    /// `resolve_plunder_age` is the EXACT function `ActionClass::
    /// PlayAggression`'s arm calls (`apply_one`, `let card =
    /// resolve_plunder_age(card, raw_text);`) -- this test calls it
    /// directly rather than driving the full journal-line dispatch, but it
    /// is the real code path, not a re-implementation of it.
    ///
    /// This is the RED/GREEN target for this session's fix: reverting
    /// `resolve_plunder_age`'s body to `card` (a bare no-op, i.e. keeping
    /// `build_card_index`'s arbitrary pick) makes this fail, because that
    /// arbitrary pick for "Plunder" is Age I (`food_and_or_resources: 3`),
    /// not the Age III the "up to 7" text proves.
    #[test]
    fn play_aggression_resolves_plunders_age_from_the_journals_own_up_to_n_text_not_an_arbitrary_same_name_sibling() {
        let index = build_card_index();
        let (arbitrary_pick, _) = longest_known_card_prefix(&index, "Plunder").expect("Plunder is in the card index");
        // Sanity check on the premise: the arbitrary pick really is the
        // "wrong" (Age I, value 3) sibling this test needs to prove gets
        // corrected -- if `build_card_index`'s own iteration order ever
        // changes to prefer a different sibling, this assertion (not
        // `resolve_plunder_age` itself) is what should fail first.
        assert_eq!(plunder_food_and_or_resources(arbitrary_pick), Some(3));

        let text = "Purple plays Plunder against Grey Your rival loses a total of up to 7 resource \
                     and/or food (your choice). You gain that many resource and food.; ; Purple uses \
                     1 military action";
        let resolved = resolve_plunder_age(arbitrary_pick, text);
        assert_eq!(
            resolved.get().name,
            "Aggression: Plunder (III)",
            "must resolve to Age III (printed cap 7, matching the journal's own text), not Age I (printed cap 3, `build_card_index`'s arbitrary pick)"
        );
        assert_eq!(plunder_food_and_or_resources(resolved), Some(7));
    }

    #[test]
    fn resolve_plunder_age_is_a_no_op_for_a_different_aggression_card_family() {
        // `resolve_plunder_age` must not touch Raid/Enslave/Spy/Annex/
        // Infiltrate -- those are explicitly out of scope this session (see
        // the pickup plan), and misreading a stray "up to N" from some
        // OTHER card's text must never redirect a non-Plunder identity.
        let raid = CardId::by_name("Aggression: Raid (I)").unwrap();
        assert_eq!(resolve_plunder_age(raid, "Purple plays Raid against Grey destroys a building"), raid);
    }

    #[test]
    fn resolve_plunder_age_falls_back_to_the_arbitrary_pick_when_the_text_has_no_up_to_clause() {
        // Never worse than the pre-correction guess: a Plunder line whose
        // text this binary cannot parse (should not happen for a real
        // corpus line, but this is a total function) keeps the original
        // pick rather than panicking or silently discarding it.
        let index = build_card_index();
        let (arbitrary_pick, _) = longest_known_card_prefix(&index, "Plunder").expect("Plunder is in the card index");
        assert_eq!(resolve_plunder_age(arbitrary_pick, "Purple plays Plunder against Grey (garbled text)"), arbitrary_pick);
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

    /// [`lost_military_resource`]'s develop-side twin, real shape from
    /// `IllegalMove: Develop` bucket triage: `"Orange discovers Air Forces
    /// Orange loses 3 military science; Orange loses 9 science"` -- the
    /// FIRST `" loses "` clause is the `p.mil_sci_discount`-funded portion,
    /// the second is the ordinary remainder.
    #[test]
    fn lost_military_science_reads_a_loses_military_science_clause() {
        assert_eq!(
            lost_military_science("Orange discovers Air Forces Orange loses 3 military science; Orange loses 9 science"),
            Some(3)
        );
        assert_eq!(lost_military_science("Purple discovers Democracy Purple loses 17 science"), None);
    }

    #[test]
    fn lost_military_science_ignores_an_unrelated_loses_clause() {
        assert_eq!(lost_military_science("Purple loses 1 population"), None);
        assert_eq!(lost_military_science("Purple discovers Computers Purple loses 8 science"), None);
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
    fn spent_food_after_resource_reads_a_trailing_food_clause_after_a_resource_clause() {
        // Real corpus shape, game `7523070` line 143: a 2-resource Warrior
        // paid 1 native resource + 1 Trade-Routes-converted food.
        assert_eq!(spent_food_after_resource("Green builds Warrior Green spends 1 resource; Green spends 1 food"), 1);
    }

    #[test]
    fn spent_food_after_resource_ignores_a_lone_resource_clause() {
        assert_eq!(spent_food_after_resource("Purple builds Bronze Purple spends 2 resources"), 0);
    }

    #[test]
    fn spent_food_after_resource_is_zero_with_no_spends_clause_at_all() {
        assert_eq!(spent_food_after_resource("Purple builds 1 stage of Pyramids"), 0);
    }

    #[test]
    fn spent_food_after_resource_reads_the_trailing_clause_even_before_a_wonder_completed_marker() {
        // Real corpus shape, game `7522613` line 176.
        assert_eq!(
            spent_food_after_resource("Green builds 1 stage of St. Peter's Basilica Green spends 3 resources; Green spends 1 food; ; Wonder completed"),
            1
        );
    }

    /// Side A of a Trade Routes Agreement between the two seats of a duel:
    /// player 0 may use 1 food as 1 resource on its own turn.
    fn give_trade_routes_side_a(r: &mut Replayer<'_>) {
        r.state.players[0].pacts.push(crate::state::Pact {
            card: CardId::by_name("Trade Routes Agreement").expect("Trade Routes Agreement is a known card"),
            owner: 0,
            partner: 1,
            a: 0,
            b: 1,
        });
    }

    /// A human may use the pact's "1 food as 1 resource" grant even when
    /// their plain resources ALONE would already cover the cost -- keeping a
    /// resource back for something else the same turn is their choice to
    /// make. Deriving the conversion from an affordability shortfall instead
    /// skipped it entirely here and let the full cost come out of real
    /// resources, draining one this binary can never get back. The drain
    /// does not fail at this line: it surfaces as an illegal move several
    /// actions later, far from its cause. Real shape: game `7521765` round
    /// 5, "Orange spends 2 resources; Orange spends 1 food" with 3 plain
    /// resources already on hand.
    #[test]
    fn convert_trade_food_shortfall_converts_the_stated_food_even_when_resources_alone_would_cover_the_cost() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        // Round 1 offers only `Take` (see `legal.rs`'s early return), so the
        // conversion move is not yet legal there.
        r.state.round = 2;
        give_trade_routes_side_a(&mut r);
        r.state.players[0].resources = 3;
        r.state.players[0].food = 4;

        convert_trade_food_shortfall(
            &mut r,
            0,
            "Orange upgrades Philosophy to Alchemy Orange spends 2 resources; Orange spends 1 food",
            3,
        )
        .expect("converting the stated food is a legal move in this position");

        assert_eq!(r.state.players[0].resources, 4, "the 1 food the journal states must convert in");
        assert_eq!(r.state.players[0].food, 3, "and must come out of food");
    }

    /// The other half: a GENUINE shortfall was always handled correctly, and
    /// still is. This guards the pre-existing behaviour against the fix
    /// above overshooting.
    #[test]
    fn convert_trade_food_shortfall_still_covers_a_genuine_shortfall() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.round = 2;
        give_trade_routes_side_a(&mut r);
        r.state.players[0].resources = 2;
        r.state.players[0].food = 4;

        convert_trade_food_shortfall(
            &mut r,
            0,
            "Orange upgrades Philosophy to Alchemy Orange spends 2 resources; Orange spends 1 food",
            3,
        )
        .expect("converting the stated food is a legal move in this position");

        assert_eq!(r.state.players[0].resources, 3, "exactly the stated 1 food fills the gap");
        assert_eq!(r.state.players[0].food, 3);
    }

    /// A build/upgrade/wonder-stage paid ENTIRELY in converted food -- no
    /// `"spends N resources"`/`"loses N military resource"` clause on the
    /// line at all, only the food one. `total_paid_for_build` correctly
    /// stays `None` for this line (it must, for `resolve_named_card_by_
    /// effect`'s unrelated resource-discount solve), but that must not make
    /// the WHOLE stated payment look like `None` too, indistinguishable
    /// from a genuinely free build/upgrade/wonder-stage this function
    /// deliberately does nothing for. Before `stated_build_payment` existed,
    /// `convert_trade_food_shortfall`'s own `.map()` composition of the two
    /// stayed `None` here, the conversion never fired, and the caller's own
    /// `Move::WonderStep`/`Move::Build`/`Move::Upgrade` charged `true_cost`
    /// straight out of `p.resources` -- resources the real player never
    /// touched. Repro: game `7522518` round 4, `"Orange builds 1 stage of
    /// Pyramids Orange spends 1 food; ; Wonder completed"`.
    #[test]
    fn convert_trade_food_shortfall_covers_a_build_paid_entirely_in_food() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.round = 2;
        give_trade_routes_side_a(&mut r);
        r.state.players[0].resources = 3;
        r.state.players[0].food = 4;

        convert_trade_food_shortfall(&mut r, 0, "Orange builds 1 stage of Pyramids Orange spends 1 food", 1)
            .expect("converting the stated food is a legal move in this position");

        assert_eq!(r.state.players[0].resources, 4, "the converted food must land in resources");
        assert_eq!(r.state.players[0].food, 3, "and must come out of food, not be a phantom free build");
    }

    /// `stated_build_payment` itself: the three shapes it must tell apart --
    /// resource-only, all-food, and genuinely free (`total_paid_for_build`'s
    /// own doc-tested free-build case) -- confirmed RED (returned `None` for
    /// the all-food case, indistinguishable from free) by reverting
    /// `stated_build_payment` to a bare `total_paid_for_build(text).map(|base|
    /// base + spent_food_after_resource(text))` before this fix.
    #[test]
    fn stated_build_payment_tells_all_food_apart_from_genuinely_free() {
        assert_eq!(stated_build_payment("Purple builds Bronze Purple spends 2 resources"), Some(2));
        assert_eq!(
            stated_build_payment("Orange upgrades Philosophy to Alchemy Orange spends 2 resources; Orange spends 1 food"),
            Some(3)
        );
        assert_eq!(
            stated_build_payment("Orange builds 1 stage of Pyramids Orange spends 1 food"),
            Some(1),
            "paid entirely in converted food -- not the same as free"
        );
        assert_eq!(
            stated_build_payment("Purple builds 1 stage of Pyramids"),
            None,
            "no clause at all is a genuinely free build, still None"
        );
    }

    #[test]
    fn build_discount_reconciles_exactly_the_masonry_gap_and_nothing_else() {
        use crate::state::TechSlot;

        let drama = CardId::by_name("Drama").unwrap(); // Age I urban, printed cost 4
        let masonry = CardId::by_name("Masonry").unwrap();

        // With Masonry developed, `build_discount[Age I]` is 1, so a journal that
        // prints the PRE-discount 4 against the engine's 3 reconciles: 4 - 3 == 1.
        // `compute` dereferences `p.government.get()` without a NONE guard, so
        // the test player needs a real government (as every live game has).
        let mut state = blank_state();
        state.players[0].government = CardId::by_name("Despotism").unwrap();
        state.players[0].techs.insert(masonry, TechSlot { workers: 0, stored: 0 });
        assert!(
            build_discount_reconciles(&state, &state.players[0], drama, 4, 3),
            "held Masonry explains the 4-vs-3 Drama gap"
        );

        // A gap that does NOT match the held discount is still a genuine
        // unmodeled source and must not reconcile.
        assert!(!build_discount_reconciles(&state, &state.players[0], drama, 5, 3));
        assert!(!build_discount_reconciles(&state, &state.players[0], drama, 4, 4), "no gap at all");

        // Without Masonry in play there is no discount to reconcile against.
        let mut bare = blank_state();
        bare.players[0].government = CardId::by_name("Despotism").unwrap();
        assert!(!build_discount_reconciles(&bare, &bare.players[0], drama, 4, 3));
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
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].one_time_discount.pop_food = 1; // banked, but...
        r.state.players[0].food = 0; // ...not enough food to spend it
        assert_eq!(civil_life_move(&r, 0, ActionClass::IncreasePopulation, None), None);
    }

    #[test]
    fn civil_life_move_offers_pop_when_the_player_can_actually_afford_it() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].one_time_discount.pop_food = 1;
        r.state.players[0].food = 20; // plenty
        assert_eq!(civil_life_move(&r, 0, ActionClass::IncreasePopulation, None), Some(Move::Pop));
    }

    /// ENGINE BUG FIX (`IllegalMove: Take` bucket, game `7523341` round 14):
    /// when a player has BOTH a spare civil action AND the wonder's free
    /// Ocean Liners population increase still available this turn, BGO's
    /// own line says exactly which one the human used --
    /// `"<Color> increases population Ocean Liner Service used"` -- but
    /// `apply_one`'s `IncreasePopulation` arm used to try `Move::Pop` first
    /// whenever it happened to be legal, silently spending a civil action
    /// the real player never spent. That phantom charge starved a LATER
    /// `Take` on the same turn, several lines downstream, of the 1 civil
    /// action it actually still had. This is `apply_one`'s real dispatch
    /// path (not `civil_life_move`'s ordered-action bypass above), so the
    /// test goes through `apply_one` directly with a `Replayer` set up so
    /// BOTH `Move::Pop` and `Move::PopFree` are legal at once -- the exact
    /// ambiguity the old "whichever is legal" logic got wrong.
    #[test]
    fn a_pop_line_naming_ocean_liner_service_used_takes_the_free_move_even_when_a_paid_pop_is_also_legal() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.round = 5;
        r.state.phase = Phase::Actions;
        r.state.current = 0;
        r.state.players[0].civil_actions = 4; // Move::Pop would also be legal
        r.state.players[0].food = 20; // plenty, for either move
        r.state.players[0].yellow_bank = 1; // pop_cost needs a nonzero bank
        r.state.players[0].completed_wonders.push(card("Ocean Liners")); // grants free_pop_per_turn
        r.state.players[0].ocean_liners_used = false;
        let out = apply_one(
            &mut r,
            0,
            ActionClass::IncreasePopulation,
            None,
            "increases population Ocean Liner Service used",
            "Orange increases population Ocean Liner Service used",
            None,
        );
        assert!(out.is_ok(), "{out:?}");
        assert_eq!(r.state.players[0].civil_actions, 4, "the free Ocean Liners move must not spend a civil action");
        assert!(r.state.players[0].ocean_liners_used, "PopFree, not Pop, must be the move actually applied");
    }

    /// `is_pure_confirmation_line`'s membership is what routes `PlayEvent`,
    /// `WinAuction`, and `Colonize` lines around `resolve_intervening`
    /// The residual, genuinely-contradictory shape: player 0 starts
    /// (`game::new_game`, round 1, before any end-of-turn draw) with
    /// exactly one Warriors worker and an EMPTY military hand, so
    /// `interact::max_force` computes exactly 1 and
    /// [`Replayer::ground_bid_ceiling`] has no filler card to convert. A
    /// bid of 3 against a standing high bid of 2 is then unreachable under
    /// any hand at all -- reported as `UnrecoverableHiddenInfo` (a
    /// contradiction between the journal and this binary's own state), not
    /// as the plain `IllegalMove` an engine defect would produce.
    #[test]
    fn a_bid_no_possible_hand_could_have_paid_for_stays_an_honest_mismatch() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
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

    /// Companion to the test above: reverting `bid_exceeds_ceiling` to
    /// `None` (i.e. deleting the reclassification and always keeping
    /// `try_apply`'s own `IllegalMove`) must turn this same fixture back
    /// into a plain `IllegalMove` -- confirming the test actually exercises
    /// the new code path rather than passing for an unrelated reason.
    #[test]
    fn without_the_reclassification_the_same_fixture_would_be_a_bare_illegal_move() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
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
    fn is_pure_confirmation_line_is_true_only_for_play_event_win_auction_colonize_and_win_war() {
        assert!(is_pure_confirmation_line(ActionClass::PlayEvent));
        assert!(is_pure_confirmation_line(ActionClass::WinAuction));
        assert!(is_pure_confirmation_line(ActionClass::Colonize));
        assert!(is_pure_confirmation_line(ActionClass::WinWar));
        assert!(!is_pure_confirmation_line(ActionClass::Pass));
        assert!(!is_pure_confirmation_line(ActionClass::Bid));
        assert!(!is_pure_confirmation_line(ActionClass::Discard));
    }

    /// REGRESSION (found by replaying real BGO game `7523809`): pins the
    /// exact failure `WinWar`'s inclusion in `is_pure_confirmation_line`
    /// exists to avoid. `game::start_turn`'s own doc: war RESOLUTION fires
    /// automatically at the START of the attacker's NEXT turn, not from the
    /// `"<Color> wins War over ..."` confirmation line -- which BGO can
    /// print with the SAME timestamp as, and immediately before, a
    /// completely unrelated OTHER player's own trailing `"End turn"` line
    /// (no `EndTurn` in between). If `resolve_intervening` is (wrongly)
    /// called for that confirmation line -- i.e. if a future edit ever
    /// removes `WinWar` from `is_pure_confirmation_line` -- the line's own
    /// named winner (here Orange, decider 0) becomes `expected_actor` while
    /// `decider()` is still whoever's turn is genuinely in progress (Purple,
    /// 1) with no pending open to explain the gap, producing exactly the
    ///    generic `StuckPending: decider != expected actor ... no pending` this
    ///    project spent a whole pass chasing.
    #[test]
    fn resolve_intervening_would_wrongly_stall_on_a_win_war_confirmation_line_reached_mid_a_different_players_turn() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        // Player 1 (Purple) is mid-turn -- some other player (0, Orange)
        // just won a war whose resolution already applied automatically
        // (`game::start_turn`'s own doc); nothing is pending, `decider()`
        // is still Purple.
        r.state.current = 1;
        r.state.phase = Phase::Actions;
        assert_eq!(r.state.decider(), 1);
        // `expected_actor: 0` mirrors a `"Orange wins War over ..."` line
        // reached while Purple (1) is still the real decider -- exactly
        // what `replay_game`'s main loop would pass if `WinWar` were ever
        // (wrongly) treated as needing `resolve_intervening` at all.
        let result = r.resolve_intervening(0, (ActionClass::WinWar, None), false);
        assert!(
            matches!(result, Err(MismatchKind::StuckPending(_))),
            "expected StuckPending (no pending, decider still mid-turn) when resolve_intervening IS \
             called for a WinWar line -- confirms is_pure_confirmation_line must keep skipping it, got {result:?}"
        );
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
    /// expected_actor (1)` sends it straight into the `Pending::Auction`
    /// handling above, which (since `decider != expected_actor`, so this is
    /// not player 0's own real response) applies a FAKE `Move::BidPass` for
    /// player 0 on the spot: this test confirms that is exactly what
    /// happens when reached directly. Before `bid_ceiling_mismatch`'s
    /// sibling fix (`Pending::Colonize` now drains unconditionally, not
    /// only once `decider` happens to stop matching `expected_actor`), the
    /// consequence was silent: player 1's newly-opened colonize sat
    /// undrained and this call still reported `Ok`. Now it drains
    /// immediately, control returns to `state.current` (player 0, who
    /// never really acted), and `decider (0) != expected_actor (1)` with an
    /// empty, non-Politics pending surfaces as a loud `StuckPending` --
    /// worse in the sense that it stops a game, but honest instead of
    /// silently corrupting one, and it is still exactly why
    /// `replay_game`'s main loop must never reach this call for a
    /// `WinAuction` line at all (skipping it, as `is_pure_confirmation_line`
    /// makes it do, leaves the auction's `player: 0` genuinely pending for
    /// their own real, upcoming line to resolve instead).
    #[test]
    fn resolve_intervening_auto_drains_a_still_open_auction_with_a_fake_bid_pass_when_called_for_a_different_expected_actor(
    ) {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        // Mirrors the real shape found on `7522652`: player 1 already placed
        // the high bid (1 -- a fresh round-1 player's own starting Warrior is
        // their entire force, `interact::max_force` == 1), and player 0
        // (`active[1]`, `pos: 1`) is the still-outstanding decider -- if they
        // ALSO pass, player 1 becomes the sole active bidder holding the
        // high bid and wins outright.
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[1, 0], 1, 1, Some(1), 0)));
        r.state.phase = Phase::Actions;
        assert_eq!(r.state.decider(), 0); // player 0's own bid/pass is still outstanding

        // Called (wrongly) as if resolving a path toward player 1 -- the
        // shape a `WinAuction` line naming the eventual winner would create
        // if it were not excluded from this call entirely.
        let result = r.resolve_intervening(1, (ActionClass::WinAuction, Some(territory)), false);

        // Player 0's own decision was fabricated and consumed sight-unseen,
        // and player 1's colonize (now correctly auto-drained) leaves
        // control back with player 0 (`state.current`, untouched by any of
        // this) -- who this call was never actually resolving a path
        // toward, hence the loud failure.
        assert!(matches!(result, Err(MismatchKind::StuckPending(_))), "expected StuckPending, got {result:?}");
        assert!(r.state.pending.is_empty(), "the fabricated colonize should still have drained to completion");
    }

    /// REGRESSION (real BGO games `7522497` and 8 others of the corpus's 12
    /// sampled completions): BGO logs the true final turn's own "End turn
    /// <Color> scores: ..." line TWICE (`replay_game`'s own doc comment on
    /// its "Last turn"/"End of game" handling has the full shape). The FIRST
    /// copy leaves a `DiscardMilitary` choice open (the human's hand is over
    /// the limit); draining that open choice happens as a side effect of
    /// `resolve_intervening` processing the SECOND copy -- and finishing the
    /// LAST queued discard can itself resume `game::resume_end_turn`,
    /// wrap the round past `final_round_end`, and run `game::finish_game`
    /// (`game::resume_end_turn`'s own doc). At that point `state.current`
    /// has already moved on to whoever `game::advance_turn` handed the turn
    /// to next, so `decider() != expected_actor` (the player who is not
    /// actually owed anything -- the game is over) used to fall straight
    /// into the `no pending` `StuckPending` arm, turning a clean finish into
    /// a reported mismatch. `resolve_intervening` must instead recognise
    /// `state.game_over` up front and return `Ok(())`: there is nothing left
    /// to intervene on. Reverting the `if self.state.game_over { return
    /// Ok(()); }` check this test pins reproduces exactly that failure.
    #[test]
    fn resolve_intervening_returns_ok_once_the_game_is_over_even_though_decider_moved_on_and_no_longer_matches(
    ) {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        // Mirrors the shape `finish_game` leaves behind: the round already
        // wrapped to the next seat (`state.current`, here player 0) before
        // the game-over check ran, nothing is pending, and the player this
        // call is trying to resolve a path FOR (player 1, mid their own
        // now-irrelevant `EndTurn`) is not `state.current` any more.
        r.state.phase = Phase::Done;
        r.state.game_over = true;
        r.state.current = 0;
        assert_ne!(r.state.decider(), 1, "player 1 must no longer be the decider, or this test proves nothing");

        let result = r.resolve_intervening(1, (ActionClass::EndTurn, None), false);

        assert!(result.is_ok(), "a finished game has nothing left to intervene on: {result:?}");
    }

    /// REGRESSION (every one of the corpus's 12 sampled completions, before
    /// this fix: `docs/REPLAY.md`'s final-score cross-check section). This
    /// binary's card row is forced to match each observed "takes ... in
    /// hand" line directly (`ground_row_slot`), not drawn through
    /// `civil_deck`/`game::deal` -- so on a real journal its reconstructed
    /// Age III deck can go an entire game without ever emptying, even when
    /// the real one did, and `game::advance_age`'s own call to
    /// `game::set_last_round` (the ONLY thing that ever sets
    /// `state.final_round_end`, which `game::advance_turn`'s round-wrap
    /// check needs to ever call `game::finish_game`) never fires. BGO's
    /// journal states the same §12.3 fact directly ("Last turn Game ends at
    /// the end of the starting round"); `replay_game` now reads it and calls
    /// this function itself, from this module, using its own (still
    /// accurate at that point) `current`/`round`/`start_player` -- this pins
    /// that `set_last_round` is actually reachable and correct from here,
    /// not just from `advance_age`'s own already-tested call site.
    #[test]
    fn set_last_round_is_reachable_from_this_module_for_the_journals_last_turn_line() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.round = 9;
        r.state.current = r.state.start_player; // BGO logs one "Last turn" line per surviving player; whichever this module reads first pins `current` to that seat.
        game::set_last_round(&mut r.state);
        assert_eq!(r.state.final_round_end, Some(9), "the seat that triggered it IS the start player, so this round is the last");
    }

    /// REGRESSION (real BGO game `7523355`, and 71 others like it in the
    /// 1,011-game corpus): a `Pending::Colonize` has no real `Move` anywhere
    /// in the journal's vocabulary, so `decider == expected_actor` must
    /// never be read as "nothing left to resolve" while one is open --
    /// the colonizer can genuinely be up next for something else entirely
    /// (here, their own `Take`), which cannot be legal until the colonize
    /// itself drains.
    #[test]
    fn resolve_intervening_drains_the_colonizers_own_pending_colonize_even_when_they_are_also_next_up_for_something_unrelated(
    ) {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        // A second unit type (on top of player 0's starting Warrior) so the
        // very first colonize decision genuinely has TWO answers
        // (`SendUnit { card: Warriors }` or `SendUnit { card: Swordsmen }`)
        // and `interact::colonize`'s own single-option auto-resolve
        // (`colonize_auto`) cannot silently finish it before
        // `resolve_intervening` is ever called -- matching the real shape
        // (multiple still-open sacrifice options) found on `7523355` and
        // the corpus generally.
        r.state.players[0]
            .techs
            .insert(CardId::by_name("Swordsmen").expect("base game card"), crate::state::TechSlot { workers: 1, stored: 0 });
        crate::interact::colonize(&mut r.state, 0, territory, 1);
        assert!(matches!(r.state.pending.top(), Some(Pending::Colonize(_))));
        assert_eq!(r.state.decider(), 0);

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert!(r.state.pending.is_empty(), "the colonize should have drained fully, unblocking the Take");
    }

    /// REGRESSION (real BGO game `7523347`, a 4-player auction): once a
    /// bidder is outbid past their own `interact::max_force` ceiling,
    /// `BidPass` is their only legal move and BGO's UI auto-passes them
    /// with no click to log at all -- the same shape as `Pending::Defense`'s
    /// forced 0-defender `DefendDone`. `resolve_intervening` must apply
    /// that forced pass even when `decider == expected_actor`, rather than
    /// assuming a matching decider means the upcoming (unrelated) line will
    /// resolve it.
    #[test]
    fn resolve_intervening_auto_passes_a_bidder_whose_own_ceiling_no_longer_clears_the_standing_bid() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        // Player 1 gets a second unit (so THEY can actually pay a winning
        // bid of 2 once player 0 is forced out -- otherwise this fixture
        // would trip the unrelated "colonize force can never reach the
        // bid" case instead of the one under test). Player 0's own ceiling
        // stays at 1 (their starting Warrior alone); the standing bid is
        // already 2, above it, so `BidPass` is their only legal move at
        // this decision -- but the upcoming line is their own unrelated
        // `Take`, not a `Bid`/`Pass` line at all.
        r.state.players[1]
            .techs
            .insert(CardId::by_name("Swordsmen").expect("base game card"), crate::state::TechSlot { workers: 1, stored: 0 });
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[0, 1], 0, 2, Some(1), 0)));
        r.state.phase = Phase::Actions;
        assert_eq!(r.state.decider(), 0);

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "expected Ok (a forced pass), got {result:?}");
        // Player 0 has passed; player 1 is now the sole active bidder and
        // wins outright, opening THEIR colonize -- which also drains
        // unconditionally (the sibling fix above), so nothing is left
        // pending for player 0's real Take line to be blocked by.
        assert!(r.state.pending.is_empty(), "player 1's resulting colonize should also have drained");
    }

    /// Companion to the two tests above: an auction decider who genuinely
    /// still has a real raise available (more than `BidPass` is legal) must
    /// never be silently auto-passed -- that would be guessing a human's
    /// decision, not resolving a forced one. This must surface as a loud
    /// `StuckPending`, not a fabricated `Ok`.
    #[test]
    fn resolve_intervening_refuses_to_guess_when_a_real_raise_is_still_available() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let territory = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Territory)
            .expect("the base game table has at least one Territory card");
        // Player 0's own ceiling is 1; the standing bid is 0 (nobody has
        // bid yet), so raising to 1 is a real, legal option -- not forced.
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[0, 1], 0, 0, None, 0)));
        r.state.phase = Phase::Actions;

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(matches!(result, Err(MismatchKind::StuckPending(_))), "expected StuckPending, got {result:?}");
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
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
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
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;

        r.resolve_political_decision(0).expect("player 0's own logged preparation");

        assert_eq!(r.next_prep, 1);
        assert_eq!(r.state.past_events.as_slice(), &[card_index["Development of Settlement"]]);
        assert_eq!(r.state.future_events.as_slice(), &[prepared]);
        assert_eq!(r.state.players[0].culture, 2);
    }

    /// FIX (chasing the discard-phase hand-size oracle's round-4 signature,
    /// `docs/REPLAY.md`, `7522614`): a real player who prepares an event
    /// plays a card they ALREADY held -- their hand shrinks by one. Left
    /// alone, `resolve_political_decision`'s own `push(prep.card)` followed
    /// by `apply`'s removal of that same identity once `Move::PrepareEvent`
    /// applies is a WASH (net zero), permanently overcounting this binary's
    /// reconstructed hand by one card per preparation. With two SIMULATED
    /// filler cards already in hand (standing in for whatever `new_game`
    /// dealt/drew before this point, of unknown identity), preparing an
    /// event must leave exactly one of them behind, not both.
    #[test]
    fn preparing_an_event_the_player_already_held_shrinks_their_hand_by_one_not_zero() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 2 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;
        // Two SIMULATED fillers of unknown identity, standing in for
        // `economy::draw_military_step`'s own ordinary draws -- neither is
        // ever named in `plan`, so `DiscardSolver::needed_after` cannot
        // protect either one; both are fair game to sacrifice.
        let filler_a = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Tactic).expect("a Tactic card exists");
        let filler_b = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Aggression)
            .expect("an Aggression card exists");
        r.state.players[0].hand_military.push(filler_a);
        r.state.players[0].hand_military.push(filler_b);

        r.resolve_political_decision(0).expect("player 0's own logged preparation");

        assert_eq!(
            r.state.players[0].hand_military.len(),
            1,
            "started with 2 fillers, prepared a card the player already held: net -1, not net 0 -- \
             hand={:?}",
            r.state.players[0].hand_military.as_slice()
        );
        assert!(
            r.state.players[0].hand_military.contains(filler_a) || r.state.players[0].hand_military.contains(filler_b),
            "the one remaining card must be one of the two original fillers, not the just-prepared card"
        );
    }

    /// FIX (real repro game `7522634`, round 3, decider 0): the SAME wash
    /// as the test just above, but with NO disposable filler at all --
    /// `resolve_political_decision`'s own "no victim" branch used to drop
    /// the decrement silently (net zero, permanently overcounting the
    /// reconstructed hand). It must now record a signed deficit instead,
    /// and a later real hand-growth (an end-of-turn draw, simulated here by
    /// directly pushing two freshly-drawn fillers the way `try_apply`'s own
    /// growth-detection loop would observe) must repay that debt rather
    /// than stacking a phantom card on top of it.
    #[test]
    fn preparing_an_event_with_no_disposable_filler_records_a_deficit_that_a_later_draw_repays() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 2 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;
        // No fillers seeded at all -- the exact `7522634` round-3 shape
        // (an empty simulated hand at that point in the real game).

        r.resolve_political_decision(0).expect("player 0's own logged preparation");

        assert_eq!(
            r.state.players[0].hand_military.len(),
            0,
            "no victim to sacrifice, and the prepared card itself leaves the hand again once `Move::PrepareEvent` \
             applies -- ends back at 0, the same net-zero as before this fix"
        );
        assert_eq!(
            r.military_hand_deficit[0], 1,
            "the dropped decrement must be recorded as an owed debt, not silently lost"
        );

        // A later real draw of two cards arrives (mirrors `try_apply`'s own
        // per-seat growth detection around `apply::apply`).
        let drawn_a = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::War).expect("a War card exists");
        let drawn_b = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Pact).expect("a Pact card exists");
        r.state.players[0].hand_military.push(drawn_a);
        r.state.players[0].hand_military.push(drawn_b);
        r.repay_military_hand_deficit(0, 2);

        assert_eq!(
            r.state.players[0].hand_military.len(),
            1,
            "two cards drawn, one immediately repays the earlier wash: net +1, not +2"
        );
        assert_eq!(r.military_hand_deficit[0], 0, "the debt is now fully repaid");
    }

    /// FIX (chasing the 103-game `SimulatorBug`/`PrepareEvent` ledger
    /// bucket, `analysis/worker_notes_2026-08-14/simbug__SIMBUG.txt`: 81 of
    /// those 103 showed the reconstructed hand one card too MANY, the
    /// classic unpaid-`military_hand_deficit` shape): unlike the sibling
    /// test just above, where the later repaying draw arrives on a
    /// SEPARATE, later `try_apply`-routed `Move` (an ordinary end-of-turn
    /// draw), a revealed political card can ALSO draw cards immediately,
    /// synchronously, inside `resolve_political_decision`'s OWN direct
    /// `apply::apply` call -- `events::reveal_current_event` ->
    /// `events::resolve_event` -> `apply_gains_block` -> `draw_military`,
    /// reachable for an `AllPlayers`/`StrongestPlayer`/etc. `drawMilitary
    /// Cards` block such as Development of Politics's "Each player draws 3
    /// military cards." `resolve_political_decision` is the one remaining
    /// call site that does not route through `Self::try_apply`'s own
    /// growth-detection loop, so before this fix that immediate draw could
    /// never repay a debt -- including a debt THIS SAME call just incurred
    /// one line above (an empty hand at the top of this test, exactly
    /// `preparing_an_event_with_no_disposable_filler_records_a_deficit_
    /// that_a_later_draw_repays`'s own repro shape) -- leaving a permanent
    /// phantom card sitting uncompensated in the freshly-drawn hand.
    #[test]
    fn preparing_an_event_that_immediately_reveals_a_card_drawing_military_cards_repays_its_own_fresh_deficit() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(
                5,
                0,
                "Orange plays event Orange scores 1 culture; Current event:; A / Development of Politics; \
                 Each player draws 3 military cards.; Orange draws 3 military cards; Purple draws 3 military cards",
            )],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;
        // §1.6: `new_game` leaves `military_deck` genuinely empty at Age A
        // (the ten Age A military cards are ALL dealt straight into
        // `current_events` at setup -- none are ever drawable) -- so
        // `economy::draw_military` would silently return `None` for every
        // draw at the game's real starting age, masking this test's own
        // mechanism. Advance to Age I and seed a real drawable deck, the
        // ordinary post-Age-A shape any actual "each player draws" event
        // fires in.
        r.state.age_military = crate::Age::I;
        r.state.military_deck = crate::game::build_deck(crate::Age::I, false, 2);
        // No fillers seeded for player 0 (Orange, the decider) -- the "no
        // disposable filler" branch fires and records a fresh deficit of 1
        // for them in this exact call, before the revealed card's own
        // immediate effect ever runs.

        r.resolve_political_decision(0).expect("player 0's own logged preparation, revealing a self-drawing event");

        assert_eq!(
            r.military_hand_deficit[0], 0,
            "Development of Politics' own immediate 'each player draws 3' effect grows player 0's hand inside \
             THIS SAME call -- it must repay the deficit this same call just incurred, not leave it owed"
        );
        assert_eq!(
            r.state.players[0].hand_military.len(),
            2,
            "3 cards drawn, 1 immediately spent repaying the fresh deficit: net +2, not +3 (a permanent phantom \
             card) and not +0 (a repay that fired twice)"
        );
        // Player 1 (Purple) is the OTHER `AllPlayers` recipient on the same
        // line, with no deficit of their own -- confirms the growth-check
        // this fix adds covers every seat the direct `apply::apply` call
        // can grow, not just the decider, and that a zero-owed repay is a
        // true no-op.
        assert_eq!(
            r.military_hand_deficit[1], 0,
            "player 1 never owed anything -- their own repay call must be a no-op"
        );
        assert_eq!(r.state.players[1].hand_military.len(), 3, "player 1 just draws their full 3 cards, untouched");
    }

    /// FIX (found chasing game `7522967`'s `SimulatorBug`/`PrepareEvent`
    /// ledger bucket -- `analysis/worker_notes_2026-08-14/handmil__HANDMIL.
    /// txt`): the sibling test just above (`preparing_an_event_that_
    /// immediately_reveals_a_card_drawing_military_cards_repays_its_own_
    /// fresh_deficit`) starts the decider's OWN debt at zero, so the ONE unit
    /// this call's own "no disposable filler" branch adds is always small
    /// enough that `owed = deficit.min(growth)` lands on the same answer
    /// whether `growth` is measured correctly or undercounted by one -- it
    /// cannot tell the two apart. This test starts the decider with a
    /// PRE-EXISTING debt of 2 (simulating an earlier, unrelated no-filler
    /// wash) on top of the fresh one this call's own empty hand adds (debt
    /// -> 3), so the fix and the bug disagree on the answer: reading `hand_
    /// military_len_before` AFTER the grounding `push` (the bug) measures
    /// this call's own growth as 2 (the push's own +1 cancels against `apply`'s
    /// -1 removal of the same identity, invisibly baked into the "before"
    /// snapshot), so `owed = min(3, 2) = 2` -- one unit of debt is left
    /// permanently owed even though the decider's hand really did grow by 3
    /// raw cards drawn minus the 1 net spent grounding this preparation (net
    /// +3), enough to cover the whole debt. Reading the snapshot before ANY
    /// mutation (the fix) measures growth correctly as 3, so `owed = min(3,
    /// 3) = 3` -- the debt clears completely.
    #[test]
    fn a_preexisting_deficit_is_fully_repaid_by_a_preparations_own_immediate_all_players_draw() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(
                5,
                0,
                "Orange plays event Orange scores 1 culture; Current event:; A / Development of Politics; \
                 Each player draws 3 military cards.; Orange draws 3 military cards; Purple draws 3 military cards",
            )],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;
        r.state.age_military = crate::Age::I;
        r.state.military_deck = crate::game::build_deck(crate::Age::I, false, 2);
        // A debt of 2 already on the books BEFORE this preparation, from
        // some earlier, unrelated no-filler wash this test does not model
        // directly (the sibling test above shows how one such wash arises).
        r.military_hand_deficit[0] = 2;
        // No fillers seeded for player 0 (Orange, the decider) here either --
        // this preparation's own grounding adds ONE MORE unit of debt (2 ->
        // 3) before the revealed card's immediate draw ever runs.

        r.resolve_political_decision(0).expect("player 0's own logged preparation, revealing a self-drawing event");

        assert_eq!(
            r.military_hand_deficit[0], 0,
            "this call's own immediate 'each player draws 3' effect grows the decider's hand by 3 net -- enough \
             to clear the full 3-unit debt (2 pre-existing + 1 fresh), not just the 2 units an undercounted \
             growth measurement would cover"
        );
        assert_eq!(
            r.state.players[0].hand_military.len(),
            0,
            "3 cards drawn, all 3 immediately spent repaying the full debt: net +0, not +1 (one unit of debt \
             left stranded by an undercounted growth measurement)"
        );
    }

    /// FIX (traced on real game `7523376`, round 10-11): `repay_military_
    /// hand_deficit` has no rule tying its debt to any particular card --
    /// unlike `resolve_political_decision`'s own preparation cost
    /// (RULES_SPEC 5.2: "choose a green military card with the harp symbol
    /// (event or territory) from hand"), which DOES. When both an
    /// Event/Territory card and a non-Event/Territory card are equally
    /// unprotected, spending the debt on the harp-symbol card first can
    /// strand a LATER, real preparation with no rule-legal card left even
    /// though this reconstruction's hand still counts enough cards overall
    /// -- confirmed on `7523376`: a repay firing right before that game's
    /// own "International Agreement" preparation picks the same `Event`
    /// card the preparation needed, forcing the preparation to fall back to
    /// an Aggression card instead. Repay must prefer the non-harp card.
    #[test]
    fn repaying_a_military_hand_deficit_prefers_a_non_harp_symbol_victim_over_an_event_or_territory_card() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(&[], &card_index, 2).expect("an empty journal is trivially consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.military_hand_deficit[0] = 1;
        // A hand with exactly one card of each kind, neither played by name
        // later (`needed_later` empty here) -- both are equally eligible by
        // the OLD rule ("any unprotected card"), only the non-harp one
        // should be eligible by the fix's own preference.
        let harp = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Event).expect("an Event card exists");
        let non_harp = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::War).expect("a War card exists");
        r.state.players[0].hand_military.push(harp);
        r.state.players[0].hand_military.push(non_harp);

        r.repay_military_hand_deficit(0, 1);

        assert!(
            r.state.players[0].hand_military.contains(harp),
            "the Event card must survive -- it is the harp-symbol card a future preparation might still need"
        );
        assert!(
            !r.state.players[0].hand_military.contains(non_harp),
            "the non-harp card is the one repay should have spent instead"
        );
        assert_eq!(r.military_hand_deficit[0], 0, "the debt is still fully repaid either way");
    }

    /// FIX (`docs/REPLAY.md`'s "Final scores" section, the mechanism traced
    /// on real games `7522166`/`7522625`): `game::auto_skip_politics` can
    /// close a player's Politics phase (`phase = Actions`, `politics_done =
    /// true`) BEFORE their real, journal-observed preparation is ever read,
    /// when this reconstruction's own `hand_military` is under-tracked and
    /// missing the exact Event/Territory card the real human held (a known,
    /// separate gap -- `legal::legal_moves` then offers only `PolPass`).
    /// This simulates exactly that: `state.phase`/`politics_done` set as if
    /// `game::auto_skip_politics` already ran (which happens synchronously,
    /// deep inside a PRIOR `apply::apply` call this test never needs to
    /// drive, via `game::advance_turn` -> `game::start_turn`), with a
    /// solved plan proving player 0's preparation line has already been
    /// reached. Before this fix, the phase stayed closed forever: the
    /// player's own `"plays event"` line is a pure confirmation
    /// (`is_pure_confirmation_line(ActionClass::PlayEvent)`) that never
    /// itself calls `resolve_intervening`, so nothing was left to notice
    /// `Phase::Politics` had already been abandoned, and the preparation
    /// was silently dropped (never popped off `current_events`, later
    /// firing a second, wrong-amount time via `events::
    /// evaluate_final_events`). `resolve_intervening` must detect this --
    /// the identical signal `GameResult::politics_false_skips` already
    /// counts -- and reopen the phase so the ordinary, already-trusted
    /// `resolve_political_decision` path (which grounds the missing card
    /// into `hand_military` itself, `ground_bid_ceiling`'s own "pop one
    /// card of unknown provenance" convention) claims it exactly like any
    /// on-time preparation would.
    #[test]
    fn resolve_intervening_reopens_a_politics_phase_auto_skip_wrongly_closed_and_claims_the_stranded_preparation() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 2 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10; // well past the preparation's own line (5)
        r.state.phase = Phase::Actions;
        r.state.players[0].politics_done = true;
        assert_eq!(r.state.decider(), 0);

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(r.politics_false_skips, 1, "the false skip must still be counted, exactly as before this fix");
        assert_eq!(r.next_prep, 1, "the stranded preparation must be claimed, not left stranded forever");
        assert_eq!(r.state.past_events.as_slice(), &[card_index["Development of Settlement"]]);
        assert_eq!(r.state.players[0].culture, 2, "the real preparation's own culture score must land");
        assert_eq!(r.state.phase, Phase::Actions, "politics is closed again once the claimed preparation resolves, same as any on-time one");
    }

    /// Companion: with NO filler in hand at all (an edge case `new_game`
    /// itself cannot actually produce, but a safe one to pin), there is
    /// nothing to sacrifice -- the old net-zero behaviour is left alone
    /// rather than underflowing or panicking.
    #[test]
    fn preparing_an_event_with_no_filler_in_hand_leaves_the_old_net_zero_behaviour_alone() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 2 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;
        r.state.players[0].hand_military = crate::state::CardList::new();

        r.resolve_political_decision(0).expect("player 0's own logged preparation, no filler to sacrifice");

        assert_eq!(r.state.players[0].hand_military.len(), 0, "nothing to pop -- the wash is a wash, not an underflow");
    }

    /// FOODFIX: `resolve_political_decision`'s `PrepareEvent` handling used
    /// to apply `events::food_or_resources`'s fixed-formula guess
    /// synchronously and then POST-HOC patch `p.food`/`p.resources` against
    /// the journal (the correction this whole section originally tested).
    /// It now opens a REAL `Pending::Choice(FoodOrResSplit)` instead (the
    /// engine-side fix -- see `state.rs`'s doc on that `ChoiceKind`), drained
    /// by `resolve_intervening`'s own `FoodOrResSplit` arm against the SAME
    /// `produces_grants`/`spends_grants` FIFOs the old post-hoc correction
    /// read. These tests enqueue the choice directly (`QueueItem::
    /// FoodOrResSplit` + `run_queue`, exactly like the sibling `LosePop`
    /// tests just above do for that `ChoiceKind`) and drive
    /// `resolve_intervening` itself, rather than reimplementing its own
    /// drain logic a second time -- the real production arm is what gets
    /// exercised, not a hand-written stand-in for it.
    ///
    /// REGRESSION (chasing the `IllegalMove: Pop` bucket, game `7523357`):
    /// Foray's `Special::StrongestPlayers` + `Gain(food_and_or_resources:
    /// 3)` grant used to resolve through the old fixed "resources first"
    /// formula -- but the real BGO line for this exact game reads `"Grey
    /// produces 2 food; Grey produces 1 resource"` while `blue_available`
    /// had 13 tokens free the whole time (not a capacity effect: a genuine
    /// human choice the old formula never asked).
    #[test]
    fn foray_resolves_the_journals_own_food_or_resources_split_not_the_deterministic_guess() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.players[0].food = 5;
        r.state.players[0].resources = 5;
        r.state.players[0].blue_total = 20; // plenty of tokens -- not a capacity effect
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::FoodOrResSplit { player: 0, amount: 3, lose: false });
        crate::interact::run_queue(&mut r.state);
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.player == 0 && matches!(c.kind, ChoiceKind::FoodOrResSplit { lose: false })),
            "a real gain-direction choice must be open: {:?}",
            r.state.pending.top()
        );
        // The journal's own resolution: 2 food, 1 resource (summing to the
        // SAME 3 total `food_and_or_resources` grants) -- the deterministic
        // formula would instead put all 3 into resources.
        r.produces_grants.insert(0, VecDeque::from([(2, 1)]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the FoodOrResSplit choice must be fully resolved, not left open");
        assert_eq!(r.state.players[0].food, 7, "5 + the journal's own 2 food, not the deterministic 0");
        assert_eq!(r.state.players[0].resources, 6, "5 + the journal's own 1 resource, not the deterministic 3");
        assert!(r.produces_grants[&0].is_empty(), "the journal-observed split is consumed, not left for a later event");
    }

    /// [`foray_resolves_the_journals_own_food_or_resources_split_not_the_
    /// deterministic_guess`]'s LOSS-side mirror (game `7522886`): Raiders'
    /// `Lose(food_and_or_resources)` block opens a real `ChoiceKind::
    /// FoodOrResSplit { lose: true }` choice, resolved against
    /// `spends_grants`. Enqueued directly for player 1 while `expected_actor`
    /// is player 0 -- exactly `resolve_intervening_drains_a_lose_pop_
    /// pending_open_for_a_different_player_than_expected_actor` above,
    /// covering the same "the targeted player need not be the decider"
    /// shape `FoodOrResSplit` inherits from `LosePop`.
    #[test]
    fn raiders_resolves_the_journals_own_food_or_resources_loss_split_not_the_deterministic_guess() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.players[1].food = 5;
        r.state.players[1].resources = 5;
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::FoodOrResSplit { player: 1, amount: 2, lose: true });
        crate::interact::run_queue(&mut r.state);
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.player == 1 && matches!(c.kind, ChoiceKind::FoodOrResSplit { lose: true })),
            "a real loss-direction choice must be open for player 1: {:?}",
            r.state.pending.top()
        );
        // The journal's own resolution: all 2 as food, protecting resources
        // entirely -- the deterministic formula would instead drain
        // resources first (5 -> 3, food untouched).
        r.spends_grants.insert(1, VecDeque::from([(2, 0)]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the FoodOrResSplit choice must be fully resolved, not left open");
        assert_eq!(r.state.players[1].food, 3, "5 - the journal's own 2 food, not the deterministic 0");
        assert_eq!(r.state.players[1].resources, 5, "resources untouched, not the deterministic 3");
        assert!(r.spends_grants[&1].is_empty(), "the journal-observed split is consumed, not left for a later event");
    }

    /// REGRESSION (chasing the `IllegalMove: Pop` bucket's residual
    /// food-short signature after the age-advance fix, game `7523052` round
    /// 9): `prescan_produces_grants` queues EVERY standalone `"<Color>
    /// produces N food[; ...]"` line in the whole journal, not just the ones
    /// this correction ever consumes -- an `AllPlayers`-shaped grant (e.g.
    /// Development of Markets, "gains 2 resources or 2 food, player's
    /// choice") resolves through a real `Pending::Choice` (`ChoiceKind::
    /// FoodOrRes`) that never reads `produces_grants` at all, so its own
    /// line can sit in the FIFO ahead of a real Foray split. The consumer
    /// arm scans by POSITION and removes only the match (`Vec::remove`, not
    /// `pop_front`), so a foreign non-matching entry is left in place for
    /// its own real consumer rather than discarded.
    #[test]
    fn foray_skips_a_foreign_non_matching_entry_queued_ahead_of_its_own_real_split() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.players[0].food = 5;
        r.state.players[0].resources = 5;
        r.state.players[0].blue_total = 20;
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::FoodOrResSplit { player: 0, amount: 3, lose: false });
        crate::interact::run_queue(&mut r.state);
        // Front: a foreign entry from an earlier, unrelated `AllPlayers`
        // grant (total 2, never one of this Foray's own total-3 options).
        // Behind it: the real Foray split for THIS event (2 food, 1 resource).
        r.produces_grants.insert(0, VecDeque::from([(2, 0), (2, 1)]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert_eq!(r.state.players[0].food, 7, "5 + the real entry's 2 food, not the deterministic 0");
        assert_eq!(r.state.players[0].resources, 6, "5 + the real entry's 1 resource, not the deterministic 3");
        assert_eq!(
            r.produces_grants[&0].as_slices().0,
            &[(2, 0)],
            "the foreign front entry is skipped in place, not consumed or reordered"
        );
    }

    /// `resolve_political_decision`'s `PrepareEvent` handling must not open
    /// a `FoodOrResSplit` choice at all for an ordinary event with no
    /// `Special::StrongestPlayers`/`WeakestPlayers` + `food_and_or_resources`
    /// shape -- a `produces_grants` entry queued for some OTHER mechanism
    /// must be left untouched.
    #[test]
    fn a_produces_grants_entry_is_left_untouched_when_the_revealed_card_is_not_a_food_or_resources_grant() {
        let card_index = build_card_index();
        let plan = crate::event_plan::solve(
            &[(5, 0, "Orange plays event Orange scores 1 culture; Current event:; A / Development of Settlement; x")],
            &card_index,
            2,
        )
        .expect("a one-preparation journal is consistent");
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.current_lineno = 10;
        r.state.phase = Phase::Politics;
        r.state.players[0].food = 5;
        r.state.players[0].resources = 5;
        // Development of Settlement grants a free population increase, not
        // food/resources -- this entry belongs to an unrelated GainBlock
        // choice elsewhere and must be left alone.
        r.produces_grants.insert(0, VecDeque::from([(2, 1)]));

        r.resolve_political_decision(0).expect("player 0's own logged preparation");

        assert!(r.state.pending.is_empty(), "Development of Settlement opens no FoodOrResSplit choice");
        assert_eq!(r.state.players[0].food, 5, "Development of Settlement never touches food");
        assert_eq!(r.state.players[0].resources, 5, "Development of Settlement never touches resources");
        assert_eq!(r.produces_grants[&0].len(), 1, "the unrelated entry is left in the FIFO for its real consumer");
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
        let mut r = Replayer::new(&card_index, 2, plan, HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
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

    /// `docs/REPLAY.md`'s "Winston Churchill's once-per-turn choice" section:
    /// real game `7522614`'s running culture total (`sources/bgo`'s own
    /// journal, `"End turn ... (now N)"`) matched this reconstruction's
    /// EXACTLY for 16 straight rounds, then fell behind by exactly 3 the
    /// instant Churchill was elected and his own "scores 3 culture." prefix
    /// first appeared on an "End turn" line -- and fell behind by another 3
    /// the very next time it appeared, never recovering either deficit for
    /// the rest of the game. `apply_churchill_end_turn_choice` is the fix:
    /// this test reproduces the mechanism directly, without a whole journal.
    #[test]
    fn apply_churchill_end_turn_choice_scores_3_culture_when_the_end_turn_line_carries_his_prefix() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.current = 0;
        r.state.phase = Phase::Actions;
        // Round 1 is the §1.9 "taking cards is the only legal action" round
        // (`legal::action_moves`'s own early return) -- Churchill is an Age
        // III leader, never in play that early in a real game, so this test
        // sets a realistic later round rather than tripping that guard.
        r.state.round = 17;
        r.state.players[0].leader = crate::CardId::by_name("Winston Churchill").expect("Churchill is in the table");
        let before = r.state.players[0].culture;

        apply_churchill_end_turn_choice(
            &mut r,
            "End turn Winston Churchill scores 3 culture.; Orange scores:; ; 5 culture (now 56); \
             6 science (now 17); 3 food - consumption: 3 (now 8); 5 resources (now 10)",
        )
        .expect("Churchill's own choice is legal on his owner's turn");

        assert_eq!(r.state.players[0].culture, before + 3);
        assert!(r.state.players[0].churchill_used, "the once-per-turn choice is spent");
    }

    /// The far more common case -- an ordinary "End turn" line with no
    /// Churchill prefix at all -- must stay a pure no-op: nothing here
    /// should ever score culture, spend the choice, or reject the line, for
    /// a player who does not even have Churchill in play.
    #[test]
    fn apply_churchill_end_turn_choice_is_a_no_op_for_an_ordinary_end_turn_line() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.current = 0;
        r.state.phase = Phase::Actions;
        let before = r.state.players[0].culture;

        apply_churchill_end_turn_choice(
            &mut r,
            "End turn Orange scores:; ; 2 culture (now 4); 2 science (now 9); \
             1 food - consumption: 1 (now 4); 3 resources (now 5)",
        )
        .expect("no Churchill prefix means nothing to apply");

        assert_eq!(r.state.players[0].culture, before);
        assert!(!r.state.players[0].churchill_used);
    }

    /// `maybe_synthesize_churchill_military`'s reason for existing: BGO
    /// never announces Churchill's MILITARY choice as its own line (unlike
    /// the culture choice's "End turn ... scores 3 culture." prefix above)
    /// -- the ONLY observable evidence is a LATER Build/Upgrade/Develop
    /// line's own `"loses N military resource"` clause. Real shape from the
    /// `IllegalMove: Build` bucket, game `7522612`: a Churchill-led player
    /// with an EMPTY `mil_discount` pool builds a unit whose journal line
    /// shows `"loses 2 military resource"` -- before this fix nothing ever
    /// filled the pool, so the build was rejected as illegal even though the
    /// human plainly just made this exact move.
    #[test]
    fn maybe_synthesize_churchill_military_fills_the_resource_pool_from_a_build_lines_evidence() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.current = 0;
        r.state.phase = Phase::Actions;
        r.state.round = 17; // Churchill is Age III, never legitimately in play in round 1
        r.state.players[0].leader = crate::CardId::by_name("Winston Churchill").expect("Churchill is in the table");
        assert_eq!(r.state.players[0].mil_discount, 0, "no prior grant this turn");

        maybe_synthesize_churchill_military(&mut r, 0, "Orange builds Warrior Orange loses 2 military resource")
            .expect("Churchill's military choice is legal on his owner's turn");

        assert_eq!(r.state.players[0].mil_discount, 3, "the whole once-per-turn grant, not just the 2 this line needed");
        assert_eq!(r.state.players[0].mil_sci_discount, 3, "the SAME choice fills both pools at once");
        assert!(r.state.players[0].churchill_used, "the once-per-turn choice is spent");
    }

    /// A player already holding enough pool from an UNRELATED source (Wave
    /// of Nationalism / Military Build-Up / Patriotism, all flat/`mil_
    /// discount`-only grants) needs no synthesis -- and synthesizing anyway
    /// would wrongly mark `churchill_used`, breaking this turn's own
    /// end-of-turn Culture-choice line if that is what the human actually
    /// picked (the two choices are mutually exclusive by the rules).
    #[test]
    fn maybe_synthesize_churchill_military_is_a_no_op_when_the_pool_already_covers_the_line() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.current = 0;
        r.state.phase = Phase::Actions;
        r.state.round = 17;
        r.state.players[0].leader = crate::CardId::by_name("Winston Churchill").expect("Churchill is in the table");
        r.state.players[0].mil_discount = 6; // e.g. already granted by Wave of Nationalism this turn

        maybe_synthesize_churchill_military(&mut r, 0, "Orange builds Warrior Orange loses 2 military resource")
            .expect("a covered line is always Ok");

        assert_eq!(r.state.players[0].mil_discount, 6, "untouched -- the existing pool already covered it");
        assert!(!r.state.players[0].churchill_used, "Churchill's OWN choice was never actually exercised here");
    }

    /// A player with no Churchill in play at all must never be charged his
    /// choice just because a `"loses N military resource"` clause happens
    /// to be present -- that evidence has to come from SOME source (Wave of
    /// Nationalism etc.), just not this one.
    #[test]
    fn maybe_synthesize_churchill_military_is_a_no_op_without_churchill_in_play() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.current = 0;
        r.state.phase = Phase::Actions;
        r.state.round = 17;

        maybe_synthesize_churchill_military(&mut r, 0, "Orange builds Warrior Orange loses 2 military resource")
            .expect("no Churchill means nothing to synthesize, never an error");

        assert_eq!(r.state.players[0].mil_discount, 0);
        assert!(!r.state.players[0].churchill_used);
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
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
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

    /// REPLAYER BUG (found chasing the `IllegalMove: Pop` bucket, game
    /// `7522619`): a forced "lose 1 population" with no free worker to
    /// absorb it (`interact::run_item`'s `QueueItem::LosePop` arm) opens a
    /// `ChoiceKind::LosePop` pending BGO resolves with the exact same
    /// `"<Color> destroys <Card>"` journal line as a `DestroyOwn` pending --
    /// but this dispatch arm only ever recognised `DestroyOwn`, so a real
    /// LosePop resolution fell through to a bare, illegal `Move::Destroy`
    /// (illegal because `legal::legal_moves`'s pending gate offers only
    /// `Choose` while ANY pending sits open), reported as `IllegalMove:
    /// Destroy`.
    #[test]
    fn a_destroys_line_resolves_a_lose_pop_pending_the_same_way_as_destroy_own() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        // Bronze is one of the starting techs (`game::START_TECHS`), already
        // staffed with 2 workers -- no need to insert it.
        let bronze = CardId::by_name("Bronze").expect("Bronze is in the table");
        r.state.players[0].workers_free = 0;
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::LosePop { player: 0, n: 1 });
        crate::interact::run_queue(&mut r.state);
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if matches!(c.kind, ChoiceKind::LosePop)),
            "no free worker, so a real choice must be open: {:?}",
            r.state.pending.top()
        );

        let out = apply_one(&mut r, 0, ActionClass::Destroy, Some(bronze), "destroys Bronze", "Orange destroys Bronze", None);

        assert!(out.is_ok(), "{out:?}");
        assert!(r.state.pending.is_empty(), "the LosePop choice must be fully resolved, not left open");
        assert_eq!(
            r.state.players[0].techs.get(bronze).map(|s| s.workers),
            Some(1),
            "Bronze (staffed with 2 workers as a starting tech) must lose exactly one"
        );
    }

    /// REPLAYER BUG (found chasing the `StuckPending: decider != expected
    /// actor ..., phase Actions, no pending` bucket, real games `7522649`/
    /// `7523045`/`7521377`, `docs/REPLAY.md`): BGO renders a `LosePop`
    /// resolution as `"<Color> disbands <Unit>"` (not `"destroys"`) when
    /// the surrendered worker-holder is a military unit -- the sibling test
    /// above already confirms `apply_one`'s `Destroy | Disband` arm handles
    /// both verbs identically once it is reached, but `resolve_intervening`'s
    /// OWN `ChoiceKind::LosePop` fast path (which decides whether to defer
    /// to that arm at all, for the same-line same-player case) used to check
    /// only `upcoming.0 == ActionClass::Destroy`, so a `Disband`-shaped
    /// upcoming line never took it -- it fell through to the
    /// `lose_pop_destroys` FIFO fallback instead, which (same bug,
    /// `prescan_lose_pop_destroys`) had never indexed `Disband` lines
    /// either, so the live pending either errored immediately ("no
    /// journal-observed destroy line") or silently stole an unrelated LATER
    /// `Destroy` line sharing the same `CardId`, orphaning this line's own
    /// resolution and leaving `state.current` advanced past it.
    #[test]
    fn resolve_intervening_defers_a_lose_pop_pending_to_apply_one_when_the_upcoming_line_is_a_matching_disband() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.players[0].workers_free = 0;
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::LosePop { player: 0, n: 1 });
        crate::interact::run_queue(&mut r.state);
        let warriors = CardId::by_name("Warriors").expect("Warriors is a starting military tech");
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.player == 0 && matches!(c.kind, ChoiceKind::LosePop)),
            "no free worker, so a real choice must be open: {:?}",
            r.state.pending.top()
        );

        // Same player, and the upcoming line IS this exact pending's own
        // resolution -- just spelled `Disband` (a military unit) instead of
        // `Destroy` (a civil card), which the sibling `Destroy` case above
        // already proves `apply_one` can finish correctly once reached.
        let result = r.resolve_intervening(0, (ActionClass::Disband, Some(warriors)), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.player == 0 && matches!(c.kind, ChoiceKind::LosePop)),
            "the fast path only defers to apply_one's own Destroy|Disband arm for the real line -- it must not \
             consume the pending itself: {:?}",
            r.state.pending.top()
        );
    }

    /// The gap the six-pending-kind pass's checkpoint left open for
    /// `LosePop` (`docs/REPLAY.md`): a still-open `LosePop` pending for a
    /// DIFFERENT player than `expected_actor` (found on real game
    /// `7521344` -- player 3's own political-phase event reveal opened a
    /// `LosePop` for player 3 while player 1 was up next for an unrelated
    /// `Destroy`) used to fall through to the generic `Some(Pending::
    /// Choice(c)) => StuckPending("no auto-resolution ...")` catch-all.
    /// `resolve_intervening` now drains it from `lose_pop_destroys`
    /// (out-of-journal-order lookahead, same tier as `GainBlock`/
    /// `PlunderSplit`/`DiscardMilitary`) and records the claimed line index
    /// so the main replay loop does not translate it a second time.
    #[test]
    fn resolve_intervening_drains_a_lose_pop_pending_open_for_a_different_player_than_expected_actor() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        // Player 1's own `LosePop`, exactly like the existing `DestroyOwn`-
        // shaped test above, but for the OTHER seat -- `expected_actor`
        // (below) is player 0, a different player entirely.
        r.state.players[1].workers_free = 0;
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::LosePop { player: 1, n: 1 });
        crate::interact::run_queue(&mut r.state);
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if c.player == 1 && matches!(c.kind, ChoiceKind::LosePop)),
            "player 1 has no free worker, so a real choice must be open for THEM: {:?}",
            r.state.pending.top()
        );
        let warriors = CardId::by_name("Warriors").expect("Warriors is a starting tech");
        // A real journal-observed `"<Color> destroys Warriors"` line for
        // player 1, at line index 3 -- `expected_actor` (0)'s own upcoming
        // line is unrelated (`TakeCard`), so this can only be resolved by
        // the out-of-order lookahead, not the `matches_upcoming` same-line
        // case the pre-existing `DestroyOwn`-shaped test already covers.
        r.lose_pop_destroys.insert(1, VecDeque::from([(3usize, warriors)]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the LosePop choice must be fully resolved, not left open");
        assert_eq!(
            r.state.players[1].techs.get(warriors).map(|s| s.workers),
            Some(0),
            "Warriors (staffed with 1 worker as a starting tech) must lose its only worker"
        );
        assert!(
            r.claimed_destroy_lines.contains(&3),
            "the claimed line index must be recorded so the main loop does not re-translate it"
        );
    }

    /// Companion to the test above: a queued `lose_pop_destroys` entry that
    /// does NOT match the live choice's own options (this same player's own
    /// separately-resolved, unrelated voluntary destroy -- see `prescan_
    /// lose_pop_destroys`'s doc) must be skipped, not trusted by position,
    /// and must NOT be recorded in `claimed_destroy_lines` since it still
    /// needs its own normal in-order processing later.
    #[test]
    fn resolve_intervening_skips_a_lose_pop_destroy_entry_that_does_not_match_the_live_choices_options() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.players[1].workers_free = 0;
        crate::interact::enqueue(&mut r.state, crate::state::QueueItem::LosePop { player: 1, n: 1 });
        crate::interact::run_queue(&mut r.state);
        let warriors = CardId::by_name("Warriors").expect("Warriors is a starting tech");
        // Iron is not one of player 1's starting techs at all -- not among
        // this LosePop choice's own options, so it must be skipped past
        // rather than mistakenly treated as the answer.
        let iron = CardId::by_name("Iron").expect("Iron is in the table");
        r.lose_pop_destroys.insert(1, VecDeque::from([(2usize, iron), (3usize, warriors)]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty());
        assert_eq!(r.state.players[1].techs.get(warriors).map(|s| s.workers), Some(0));
        assert!(r.claimed_destroy_lines.contains(&3), "the real, matching entry must be claimed");
        assert!(
            !r.claimed_destroy_lines.contains(&2),
            "the skipped, non-matching entry must NOT be claimed -- it still needs its own normal processing"
        );
    }

    /// `DestroyOwn` (an event's weakest-civilization `destroyOwnBuilding` --
    /// today only `Border Conflict` / `Uncertain Borders` print it): the
    /// choice is opened for the WEAKEST player, who is `expected_actor`'s
    /// neighbor when the event was revealed on a DIFFERENT player's turn
    /// (real game `7521741` line 242: Green reveals `Border Conflict`,
    /// Purple -- the weakest -- must destroy, while Orange is up next for an
    /// unrelated take). It used to fall through to the generic `Some(
    /// Pending::Choice(c)) => StuckPending("no auto-resolution ...")`
    /// catch-all. `resolve_intervening` now drains it from
    /// `lose_pop_destroys` (the SAME per-player FIFO `LosePop` uses -- both
    /// are `"<Color> destroys <Card>"` lines), validated against the live
    /// choice's own options and recorded in `claimed_destroy_lines` so the
    /// main loop does not re-translate the line.
    #[test]
    fn resolve_intervening_drains_a_destroy_own_pending_open_for_a_different_player_than_expected_actor() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let bronze = CardId::by_name("Bronze").expect("Bronze is a starting tech");
        let irrigation = CardId::by_name("Irrigation").expect("Irrigation is a known card");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(bronze));
        options.push(ChoiceOption::Card(irrigation));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::DestroyOwn, options }));
        r.lose_pop_destroys.insert(1, VecDeque::from([(3usize, bronze)]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the DestroyOwn choice must be fully resolved, not left open");
        assert!(
            r.claimed_destroy_lines.contains(&3),
            "the claimed line index must be recorded so the main loop does not re-translate it"
        );
    }

    /// Companion to the test above: with no journal-observed destroy line
    /// for the choice's player at all, `resolve_intervening` must GUESS the
    /// first Card option rather than failing loudly -- the journal does not
    /// log the building in every case, and the engine's `resolve_choice`
    /// validates the pick against the choice's own options, so a wrong guess
    /// is `IllegalMove`, not silent.
    #[test]
    fn resolve_intervening_guesses_a_destroy_own_pending_with_no_journal_observed_destroy_line() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let bronze = CardId::by_name("Bronze").expect("Bronze is a starting tech");
        let irrigation = CardId::by_name("Irrigation").expect("Irrigation is a known card");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(bronze));
        options.push(ChoiceOption::Card(irrigation));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::DestroyOwn, options }));
        // No `lose_pop_destroys` entry for player 1 -- the guess path.

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the DestroyOwn choice must be fully resolved by the guess");
        assert!(
            !r.claimed_destroy_lines.contains(&3),
            "no line was claimed -- nothing was applied out of order"
        );
    }

    /// `Raid` (Aggression: Raid / the Terrorism event): a still-open
    /// `Pending::Choice(Raid)` used to fall through to the generic
    /// `Some(Pending::Choice(c)) => StuckPending("no auto-resolution ...")`
    /// catch-all, even though the destroyed building's identity is right
    /// there in the journal -- either the Terrorism event's own
    /// `"Terrorists destroy a <Color> <Building>"` line, or Aggression:
    /// Raid's own `"Raid casualties ..."` line (`prescan_raid_destroys`).
    /// `resolve_intervening` now drains it from a GLOBAL (not per-player,
    /// since Terrorism's own line never names an attacker) FIFO, validated
    /// against the live choice's own options.
    #[test]
    fn resolve_intervening_drains_a_raid_pending_from_the_global_fifo() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let alchemy = CardId::by_name("Alchemy").expect("Alchemy is a known urban building");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(alchemy));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::Raid { victim: 1, loot: true, is_last: true }, options }));
        r.raid_destroys = VecDeque::from([alchemy]);

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the Raid choice must be fully resolved, not left open");
        assert!(r.raid_destroys.is_empty(), "the consumed entry must be popped from the global FIFO");
    }

    /// Companion to the test above: a queued `raid_destroys` entry that does
    /// NOT match the live choice's own options (belonging to an EARLIER
    /// single-candidate Raid this same game already auto-resolved with no
    /// `Pending` at all -- see `prescan_raid_destroys`'s doc) must be
    /// skipped, not trusted by position, exactly like `PlunderSplit`'s own
    /// FIFO.
    #[test]
    fn resolve_intervening_skips_a_raid_entry_that_does_not_match_the_live_choices_options() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let alchemy = CardId::by_name("Alchemy").expect("Alchemy is a known urban building");
        // Iron is not among this live choice's own options at all -- it
        // belongs to an earlier, already auto-resolved single-candidate
        // Raid this same global FIFO also carries.
        let iron = CardId::by_name("Iron").expect("Iron is in the table");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(alchemy));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::Raid { victim: 1, loot: true, is_last: true }, options }));
        r.raid_destroys = VecDeque::from([iron, alchemy]);

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty());
        assert!(
            r.raid_destroys.is_empty(),
            "both the skipped mismatch and the real matching answer must be drained from the FIFO"
        );
    }

    /// A real Raid II's two NESTED-eligibility brackets (`interact.rs`'s
    /// `QueueItem::Raid`: `id.level() <= max_lv`) must not be resolved by
    /// blind FIFO order. Repro: game `7522513` round 13, Raid II against
    /// Green -- journal line `"Raid casualties 1 Alchemy; 1 Organized
    /// Religion"` (plain alphabetical order, NOT bracket-resolution order).
    /// Organized Religion (level II) is only a legal option for the WIDE
    /// (level 0-2) bracket; Alchemy (level <=I) is legal for either. The
    /// WIDE bracket opens first (`card_table.rs`'s `DestroyUrbanBuildings(&
    /// [Age::II, Age::I])`, widest-listed-first). Taking the FIFO's front
    /// entry (Alchemy) for the wide bracket, as this code used to, leaves
    /// Organized Religion impossible for the narrow bracket that opens next
    /// (not among its options at all) -- it gets wrongly declined (`Stop`),
    /// permanently under-counting Green's `workers_free` by one. The fix:
    /// prefer the HIGHEST-level matching FIFO entry for the CURRENT
    /// bracket, since a higher level has fewer alternative homes.
    #[test]
    fn resolve_intervening_prefers_the_highest_level_raid_casualty_for_a_wide_bracket_leaving_a_lower_level_one_for_the_narrow_bracket_that_opens_next(
    ) {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let alchemy = CardId::by_name("Alchemy").expect("Alchemy is a known urban building");
        let organized_religion = CardId::by_name("Organized Religion").expect("Organized Religion is a known urban building");
        assert!(
            organized_religion.level() > alchemy.level(),
            "the repro depends on Organized Religion outranking Alchemy"
        );
        r.state.players[1].techs.insert(alchemy, crate::state::TechSlot { workers: 1, stored: 0 });
        r.state.players[1].techs.insert(organized_religion, crate::state::TechSlot { workers: 1, stored: 0 });
        // The WIDE (level 0-2) bracket: both casualties are legal options.
        let mut wide_options = crate::state::OptionList::new();
        wide_options.push(ChoiceOption::Card(alchemy));
        wide_options.push(ChoiceOption::Card(organized_religion));
        wide_options.push(ChoiceOption::Word(Keyword::Stop));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::Raid { victim: 1, loot: false, is_last: false }, options: wide_options }));
        // Journal order (alphabetical, not bracket order): Alchemy first.
        r.raid_destroys = VecDeque::from([alchemy, organized_religion]);

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert_eq!(
            r.state.players[1].techs.workers(organized_religion),
            0,
            "the wide bracket must spend its slot on the higher-level casualty"
        );
        assert_eq!(
            r.state.players[1].techs.workers(alchemy),
            1,
            "the lower-level casualty, eligible for either bracket, must be left for the narrow one"
        );
        assert_eq!(
            r.raid_destroys.as_slices().0,
            &[alchemy],
            "only the picked casualty leaves the FIFO -- the other stays for the next bracket"
        );
    }

    /// [`parse_terrorism_destroy_line`]: the Terrorism event's own
    /// destruction line names the VICTIM's colour, not an attacker -- this
    /// must still resolve the destroyed card's identity.
    #[test]
    fn parse_terrorism_destroy_line_reads_the_building_past_the_victims_colour() {
        let card_index = build_card_index();
        let scientific_method = CardId::by_name("Scientific Method").expect("Scientific Method is a known card");
        assert_eq!(
            parse_terrorism_destroy_line(&card_index, "Terrorists destroy a Purple Scientific Method"),
            Some(scientific_method)
        );
    }

    /// [`parse_raid_casualties_line`]: both the single- and double-casualty
    /// shapes (one clause per printed age tier) must resolve every
    /// destroyed building, in order, and stop at the trailing "<Attacker>
    /// produces <M> resources" clause without swallowing it as a card name.
    #[test]
    fn parse_raid_casualties_line_reads_every_casualty_in_order() {
        let card_index = build_card_index();
        let alchemy = CardId::by_name("Alchemy").expect("Alchemy is a known card");
        let opera = CardId::by_name("Opera").expect("Opera is a known card");
        assert_eq!(
            parse_raid_casualties_line(&card_index, "Raid casualties 1 Alchemy; Purple produces 3 resources"),
            Some(vec![alchemy])
        );
        assert_eq!(
            parse_raid_casualties_line(&card_index, "Raid casualties 1 Alchemy; 1 Opera; Orange produces 8 resources"),
            Some(vec![alchemy, opera])
        );
    }

    /// `LoseColony` (Independence Declaration, the sixth of the seven
    /// kinds): a still-open multi-colony `Pending::Choice(LoseColony)` used
    /// to fall through to the generic catch-all, even though the real
    /// choice's own resolution -- `"<Color> loses <Territory> (<Age
    /// numeral>)"`, a SEPARATE line from the single-colony auto-resolve's
    /// `"<Territory> declares its independence from <Color>"` (glued onto
    /// the triggering `"plays event"` line itself) -- is right there in the
    /// journal (`prescan_lose_colonies`). `resolve_intervening` now drains
    /// it from a per-actor FIFO, validated against the live choice's own
    /// options.
    #[test]
    fn resolve_intervening_drains_a_lose_colony_pending_for_the_matching_territory() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let historic_1 = CardId::by_name("Historic Territory (I)").expect("Historic Territory (I) is a known card");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(historic_1));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::LoseColony, options }));
        r.lose_colonies.insert(1, VecDeque::from([historic_1]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the LoseColony choice must be fully resolved, not left open");
        assert!(r.lose_colonies.get(&1).is_none_or(|q| q.is_empty()));
    }

    /// Companion to the test above: a queued `lose_colonies` entry that does
    /// NOT match the live choice's own options (belonging to an EARLIER
    /// single-colony case this same player already auto-resolved with no
    /// `Pending` at all) must be skipped, not trusted by position.
    #[test]
    fn resolve_intervening_skips_a_lose_colony_entry_that_does_not_match_the_live_choices_options() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let historic_1 = CardId::by_name("Historic Territory (I)").expect("Historic Territory (I) is a known card");
        // Wealthy Territory (II) is not among this live choice's own
        // options at all -- it belongs to an earlier, already auto-resolved
        // single-colony case for this same player.
        let wealthy_2 = CardId::by_name("Wealthy Territory (II)").expect("Wealthy Territory (II) is a known card");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(historic_1));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::LoseColony, options }));
        r.lose_colonies.insert(1, VecDeque::from([wealthy_2, historic_1]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty());
        assert!(
            r.lose_colonies.get(&1).is_none_or(|q| q.is_empty()),
            "both the skipped mismatch and the real matching answer must be drained"
        );
    }

    /// `WarTech` (War over Technology): the ENGINE BUG this whole pass
    /// fixes -- games `7522455`/`7522967` (real BGA journals, `docs/
    /// REPLAY.md`) both got stuck forever on this exact `Pending`, because
    /// `resolve_intervening` used to leave `ChoiceKind::WarTech` alone
    /// unconditionally (the fallthrough arm's own former comment: "`Annex`/
    /// `WarTech` have not been needed by any corpus game sampled so far").
    /// `interact::war_tech_spoils` itself already modelled mixing correctly
    /// (`combat.rs`'s own `apply_war_spoils_technology_the_victor_may_mix_
    /// cards_and_science` test) -- the gap was purely on this file's side:
    /// nothing ever translated the journal's per-pick `"takes spoils of
    /// war"` lines into the `Move::Choose`s that mixing needs. This drives
    /// the real war_tech_spoils entry point exactly like production does
    /// (not a hand-built `Pending::Choice`), so it also exercises `interact::
    /// offer_war_tech`'s re-offer-after-a-steal loop: after the steal
    /// resolves, budget 8 minus Navigation's cost 6 leaves 2, and with no
    /// second stealable special tech in play, `offer_war_tech` auto-resolves
    /// that remainder as science with NO second `Pending` -- so ONE `resolve_
    /// intervening` call must drain the whole two-pick FIFO and leave
    /// `state.pending` empty, not just the first pick.
    #[test]
    fn resolve_intervening_drains_a_war_tech_pending_split_between_a_steal_and_the_science_remainder() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let navigation = CardId::by_name("Navigation").expect("Navigation is a known special technology");
        r.state.players[1].techs.insert(navigation, crate::state::TechSlot { workers: 0, stored: 0 });
        r.state.players[1].science = 20;
        // The real production entry point (`combat::apply_war_spoils`'s own
        // caller) -- opens the live `Pending::Choice(WarTech)` exactly like
        // an actual resolved war does, budget 8 = Navigation's cost (6) plus
        // a 2-science remainder, matching the split this test's own doc
        // describes.
        crate::interact::war_tech_spoils(&mut r.state, 0, 1, 8);
        assert!(r.state.pending.top().is_some(), "war_tech_spoils must open a real choice when a steal is on offer");
        r.war_tech_spoils.insert(0, VecDeque::from([WarSpoil::Steal(navigation), WarSpoil::Science]));

        // `expected_actor: 0`, matching the victor: once both picks drain
        // and `state.pending` empties, `resolve_intervening`'s own "whose
        // turn is it really" fallback must agree with whoever is genuinely
        // up next in a freshly `Replayer::new`-constructed game (player 0),
        // exactly like the `LoseColony`/`FlipWonder` tests above use `0` for
        // the same reason -- an `expected_actor` that does not match would
        // report an unrelated `StuckPending` AFTER this choice already
        // resolved correctly, not a failure of the fix under test here.
        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "both picks must be resolved, not just the first");
        assert!(r.state.players[0].techs.has(navigation), "the stolen technology must move to the victor's play area");
        assert!(!r.state.players[1].techs.has(navigation), "and leave the loser's");
        assert_eq!(r.state.players[1].science, 18, "the loser pays the 2-science remainder after the 6-cost steal");
        assert_eq!(r.state.players[0].science, 2, "the victor gains exactly the remainder, not the whole budget");
    }

    /// `FlipWonder` (Ravages of Time, the last of the seven kinds): a still-
    /// open multi-wonder `Pending::Choice(FlipWonder)` used to fall through
    /// to the generic catch-all, even though the real choice's own
    /// resolution -- `"Ravages of Time <Wonder> crumble(s)"`, a SEPARATE
    /// line with no leading colour at all (`Line::color` is the only place
    /// the actor is) from the single-wonder auto-resolve's `"The <Wonder>
    /// crumble(s)"` glued onto the triggering `"plays event"` line -- is
    /// right there in the journal (`prescan_flip_wonders`). `resolve_
    /// intervening` now drains it from a per-actor FIFO, validated against
    /// the live choice's own options.
    #[test]
    fn resolve_intervening_drains_a_flip_wonder_pending_for_the_matching_wonder() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let pyramids = CardId::by_name("Pyramids").expect("Pyramids is a known wonder");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(pyramids));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::FlipWonder, options }));
        r.flip_wonders.insert(1, VecDeque::from([pyramids]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the FlipWonder choice must be fully resolved, not left open");
        assert!(r.flip_wonders.get(&1).is_none_or(|q| q.is_empty()));
    }

    /// Companion to the test above: a queued `flip_wonders` entry that does
    /// NOT match the live choice's own options (an earlier, already auto-
    /// resolved single-wonder case for this same player) must be skipped,
    /// not trusted by position.
    #[test]
    fn resolve_intervening_skips_a_flip_wonder_entry_that_does_not_match_the_live_choices_options() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let pyramids = CardId::by_name("Pyramids").expect("Pyramids is a known wonder");
        // Colossus is not among this live choice's own options at all.
        let colossus = CardId::by_name("Colossus").expect("Colossus is a known wonder");
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Card(pyramids));
        r.state.pending.push(Pending::Choice(Choice { player: 1, kind: ChoiceKind::FlipWonder, options }));
        r.flip_wonders.insert(1, VecDeque::from([colossus, pyramids]));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty());
        assert!(
            r.flip_wonders.get(&1).is_none_or(|q| q.is_empty()),
            "both the skipped mismatch and the real matching answer must be drained"
        );
    }

    /// [`parse_lose_colony_line`]: the territory's printed `name` already
    /// bakes the age suffix in (`"Historic Territory (I)"`), so this must
    /// resolve the EXACT card, not a bare family name.
    #[test]
    fn parse_lose_colony_line_resolves_the_exact_aged_territory_card() {
        let card_index = build_card_index();
        let historic_1 = CardId::by_name("Historic Territory (I)").expect("Historic Territory (I) is a known card");
        assert_eq!(
            parse_lose_colony_line(&card_index, "Purple loses Historic Territory (I)"),
            Some((Color::Purple, historic_1))
        );
    }

    /// [`parse_war_tech_steal_line`]: the doubled-actor BGO shape from real
    /// game `7522455` line 201, and the confirming `"wins War over
    /// Technology ... takes spoils of war"` line right before it (`7522455`
    /// line 198) -- which ends at "war" with no continuation -- must NOT be
    /// mistaken for a steal.
    #[test]
    fn parse_war_tech_steal_line_reads_the_stolen_card_and_ignores_the_winwar_confirmation() {
        let card_index = build_card_index();
        let navigation = CardId::by_name("Navigation").expect("Navigation is a known card");
        assert_eq!(
            parse_war_tech_steal_line(&card_index, "Orange takes spoils of war Orange steals Navigation from Purple"),
            Some((Color::Orange, navigation))
        );
        assert_eq!(
            parse_war_tech_steal_line(
                &card_index,
                "Orange wins War over Technology Attacker's strength: 27; Defender's strength: 14; Orange takes spoils of war"
            ),
            None
        );
    }

    /// [`parse_war_tech_science_line`]: real game `7522455` line 202 --
    /// the remainder-as-science half of the same split whose steal half
    /// [`parse_war_tech_steal_line_reads_the_stolen_card_and_ignores_the_winwar_confirmation`]
    /// covers. Only the shape is checked, not the amount (see [`WarSpoil::
    /// Science`]'s own doc), so a double-digit amount must parse identically
    /// to a single-digit one.
    #[test]
    fn parse_war_tech_science_line_reads_the_actor_regardless_of_amount_width() {
        assert_eq!(
            parse_war_tech_science_line("Orange takes spoils of war Orange gets 6 science; Purple loses 6 science"),
            Some(Color::Orange)
        );
        assert_eq!(
            parse_war_tech_science_line("Orange takes spoils of war Orange gets 11 science; Purple loses 11 science"),
            Some(Color::Orange)
        );
        assert_eq!(parse_war_tech_science_line("Orange takes spoils of war Orange steals Navigation from Purple"), None);
    }

    /// [`prescan_war_tech_spoils`]: the exact two-line split real game
    /// `7522455` logs (steal, then the remainder as science) must land in
    /// that same order in the per-actor FIFO -- `resolve_intervening`'s
    /// `ChoiceKind::WarTech` arm pops front-first, so a reversed order would
    /// silently offer the science pick before the steal one.
    #[test]
    fn prescan_war_tech_spoils_orders_a_steal_then_science_split_in_journal_order() {
        let card_index = build_card_index();
        let navigation = CardId::by_name("Navigation").expect("Navigation is a known card");
        let journal = [
            line(
                198,
                "II",
                "Orange wins War over Technology Attacker's strength: 27; Defender's strength: 14; Orange takes spoils of war",
            ),
            line(201, "II", "Orange takes spoils of war Orange steals Navigation from Purple"),
            line(202, "II", "Orange takes spoils of war Orange gets 6 science; Purple loses 6 science"),
        ];
        let mut out = prescan_war_tech_spoils(&journal, &card_index);
        let q = out.remove(&Color::Orange.seat()).expect("Orange has two spoils picks");
        assert_eq!(q, VecDeque::from([WarSpoil::Steal(navigation), WarSpoil::Science]));
    }

    /// [`parse_ravages_of_time_line`]: the actor comes from `Line::color`
    /// (column 2), not the text itself -- and a leading "The " in the
    /// flavour text (present for some wonders, absent for others, e.g. "St.
    /// Peter's Basilica") must not be mistaken for part of the card name.
    #[test]
    fn parse_ravages_of_time_line_reads_the_actor_from_column_two_and_strips_a_leading_the() {
        let card_index = build_card_index();
        let library = CardId::by_name("Library of Alexandria").expect("Library of Alexandria is a known card");
        assert_eq!(
            parse_ravages_of_time_line(&card_index, "Purple", "Ravages of Time The Library of Alexandria crumbles"),
            Some((Color::Purple, library))
        );
        let basilica = CardId::by_name("St. Peter's Basilica").expect("St. Peter's Basilica is a known card");
        assert_eq!(
            parse_ravages_of_time_line(&card_index, "Grey", "Ravages of Time St. Peter's Basilica crumbles"),
            Some((Color::Grey, basilica))
        );
    }

    /// `TakeRow` (International Agreement, `docs/REPLAY.md`'s six-pending-
    /// kind pass checkpoint): when the upcoming line is `expected_actor`'s
    /// own take of a card still among the open choice's own `Slot` options,
    /// `resolve_intervening` must defer (return `Ok(())` without touching
    /// the pending) exactly like `FreeBuild`, leaving `apply_one`'s
    /// `TakeCard` arm to translate it into the right `Choose` -- NOT auto-
    /// select `Stop` here, which would wrongly discard a real observed pick.
    #[test]
    fn resolve_intervening_defers_a_take_row_pending_when_the_upcoming_take_matches_one_of_its_slots() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let iron = CardId::by_name("Iron").expect("Iron is in the table");
        r.state.card_row[2] = iron;
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Slot(2));
        options.push(ChoiceOption::Word(Keyword::Stop));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::TakeRow { budget: 5 }, options }));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, Some(iron)), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if matches!(c.kind, ChoiceKind::TakeRow { .. })),
            "deferring must leave the pending untouched for apply_one to resolve: {:?}",
            r.state.pending.top()
        );
    }

    /// Companion: a card this binary has never grounded ANYWHERE in
    /// `card_row` yet -- no earlier "takes"/"builds" line ever named it, the
    /// routine shape for a WONDER taken via International Agreement, which
    /// `card_row` never held before this exact pick -- still defers, even
    /// though no offered `Slot` holds it by IDENTITY. Regression: the
    /// original `matches_upcoming` check required an identity match, so it
    /// always read false for this shape and silently declined the whole
    /// `TakeRow` via `Stop` before `apply_one`'s own `TakeCard` arm (which
    /// DOES know how to ground an ungrounded slot) ever got a chance to run
    /// -- corpus-confirmed against real games `7522957`/`7523114`
    /// (`Reserves`/`First Space Flight`), caught by the `IllegalMove:
    /// WonderStep` bucket's own regression bar.
    #[test]
    fn resolve_intervening_defers_a_take_row_pending_for_a_card_never_before_grounded_in_the_row() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let iron = CardId::by_name("Iron").expect("Iron is in the table");
        let first_space_flight = CardId::by_name("First Space Flight").expect("First Space Flight is in the table");
        // Slot 2 holds a DIFFERENT card, ungrounded -- `first_space_flight`
        // is not present anywhere in `card_row` at all.
        r.state.card_row[2] = iron;
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Slot(2));
        options.push(ChoiceOption::Word(Keyword::Stop));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::TakeRow { budget: 5 }, options }));

        let result = r.resolve_intervening(0, (ActionClass::TakeCard, Some(first_space_flight)), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(
            matches!(r.state.pending.top(), Some(Pending::Choice(c)) if matches!(c.kind, ChoiceKind::TakeRow { .. })),
            "deferring must leave the pending untouched for apply_one to resolve: {:?}",
            r.state.pending.top()
        );
    }

    /// Companion: when the upcoming line does NOT match any of the open
    /// `TakeRow` choice's `Slot` options (a different actor's own line, or
    /// this same actor's line but for an unrelated action/card), a human
    /// DECLINING the row leaves no journal trace -- `resolve_intervening`
    /// must auto-select `Word(Stop)` and keep going, the same "no journal
    /// trace for a silent decline" precedent `FreeBuild` already uses.
    #[test]
    fn resolve_intervening_auto_declines_a_take_row_pending_via_stop_when_the_upcoming_line_does_not_match() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let iron = CardId::by_name("Iron").expect("Iron is in the table");
        r.state.card_row[2] = iron;
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Slot(2));
        options.push(ChoiceOption::Word(Keyword::Stop));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::TakeRow { budget: 5 }, options }));

        // Player 0's own next line is an EndTurn, not a take of Iron (or
        // anything else) -- there is nothing to defer to.
        let result = r.resolve_intervening(0, (ActionClass::EndTurn, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(
            r.state.pending.is_empty(),
            "auto-declining via Stop must fully close the choice, not leave it open: {:?}",
            r.state.pending.top()
        );
    }

    /// `apply_one`'s `TakeCard` arm must translate an observed take into a
    /// `Choose` naming the matching `Slot` option when a `TakeRow` pending
    /// sits open, mirroring the pre-existing `DestroyOwn | LosePop` check in
    /// the `Destroy | Disband` arm -- a bare `Move::Take` is illegal while
    /// ANY pending sits open (`legal::legal_moves`'s pending gate), so
    /// without this the take would be reported as `IllegalMove: Take`
    /// instead of correctly clearing the choice.
    #[test]
    fn apply_one_resolves_a_take_row_pending_via_choose_instead_of_a_bare_take() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let iron = CardId::by_name("Iron").expect("Iron is in the table");
        // Already grounded in slot 2 -- `ground_row_slot`'s first (trusted)
        // branch then returns that slot directly, independent of the
        // observed cost text, exactly like a normal prior `Take` would have
        // left it.
        r.state.card_row[2] = iron;
        r.row_grounded[2] = true;
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Slot(2));
        options.push(ChoiceOption::Word(Keyword::Stop));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::TakeRow { budget: 5 }, options }));

        let out = apply_one(
            &mut r,
            0,
            ActionClass::TakeCard,
            Some(iron),
            "takes Iron in hand Orange uses 1 civil action",
            "Orange takes Iron in hand Orange uses 1 civil action",
            None,
        );

        assert!(out.is_ok(), "{out:?}");
        assert!(
            r.state.players[0].hand_civil.as_slice().contains(&iron),
            "Iron must actually land in hand: {:?}",
            r.state.players[0].hand_civil.as_slice()
        );
        assert!(
            !r.row_grounded[2],
            "the slot's refill must be ungrounded again, exactly like the ordinary Move::Take path"
        );
    }

    /// `Infiltrate` (sixth pending kind, flagged mid-pass by the concurrent
    /// Take-bucket worker as sharing the identical `decider == expected_
    /// actor` gap): a still-open `Infiltrate` choice for the ATTACKER must
    /// drain unconditionally from `infiltrates`, same tier as
    /// `PlunderSplit`, regardless of whose line `resolve_intervening` was
    /// actually called for.
    #[test]
    fn resolve_intervening_drains_an_infiltrate_pending_using_the_journal_observed_pick() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        // Player 1 (the victim) has a wonder under construction to remove --
        // observable proof that the WONDER option (not Leader) was chosen.
        let wonder = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Wonder)
            .expect("the base game table has at least one Wonder card");
        r.state.players[1].wonder = wonder;
        r.state.players[1].wonder_steps = 2;
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Word(Keyword::Leader));
        options.push(ChoiceOption::Word(Keyword::Wonder));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::Infiltrate { victim: 1, per: 3 }, options }));
        // The journal-observed resolution: an is_destroyed (wonder) pick.
        r.infiltrates.insert(0, VecDeque::from([true]));

        // expected_actor matches state.current (player 0, new_game's
        // default) so the loop can cleanly exit once the pending drains --
        // the important thing this test proves is that the Infiltrate
        // check fires unconditionally, ahead of any decider/expected_actor
        // comparison at all, exactly like PlunderSplit.
        let result = r.resolve_intervening(0, (ActionClass::EndTurn, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty(), "the Infiltrate choice must be fully resolved, not left open");
        assert!(r.state.players[1].wonder.is_none(), "the victim's wonder must be discarded, proving Wonder was picked");
        assert_eq!(
            r.state.players[0].culture,
            3 * wonder.level() as u16,
            "3 culture per level of the removed wonder card"
        );
    }

    /// Companion: a queued `infiltrates` entry that does NOT match the live
    /// choice's own options (an earlier, single-option Infiltrate this same
    /// attacker already auto-resolved silently -- see the field's own doc)
    /// must be skipped, not trusted by position.
    #[test]
    fn resolve_intervening_skips_an_infiltrate_entry_that_does_not_match_the_live_choices_options() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        let wonder = (0..crate::CARDS.len() as u16)
            .map(CardId)
            .find(|id| id.kind() == CardType::Wonder)
            .expect("the base game table has at least one Wonder card");
        r.state.players[1].wonder = wonder;
        r.state.players[1].wonder_steps = 1;
        // This choice only offers Wonder (the victim has no leader this
        // time) -- a queued `false` (Leader) entry must be skipped past.
        let mut options = crate::state::OptionList::new();
        options.push(ChoiceOption::Word(Keyword::Wonder));
        r.state.pending.push(Pending::Choice(Choice { player: 0, kind: ChoiceKind::Infiltrate { victim: 1, per: 3 }, options }));
        r.infiltrates.insert(0, VecDeque::from([false, true]));

        let result = r.resolve_intervening(0, (ActionClass::EndTurn, None), false);

        assert!(result.is_ok(), "{result:?}");
        assert!(r.state.pending.is_empty());
        assert!(r.state.players[1].wonder.is_none());
        assert!(
            r.infiltrates.get(&0).is_some_and(|q| q.is_empty()),
            "both queued entries must be consumed (one skipped, one used)"
        );
    }

    /// REPLAYER BUG (found chasing the `IllegalMove: Pop` bucket): a
    /// territory named in a `"Christopher Columbus discovers <Age> /
    /// <Territory>"` line is routinely the FIRST evidence of that specific
    /// card at all -- territories arrive via the automatic end-of-turn
    /// draw, not an observed `"takes ... in hand"` line, so `p.
    /// hand_military` still holds `new_game`'s SIMULATED filler until
    /// grounded, same as `DeclareWar`/`PlayAggression`/`ProposePact`
    /// already ground their own card right before playing it
    /// (`ground_for_consumption`'s own doc). Without grounding first,
    /// `Move::ColumbusColonize` is illegal (the territory isn't really in
    /// hand) even though the human's move was perfectly legal -- this
    /// alone was a 123-game bucket (`IllegalMove: ColumbusColonize`) the
    /// instant the line stopped being silently dropped as bookkeeping.
    #[test]
    fn a_columbus_colonize_line_grounds_the_territory_before_applying_it() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        // `Move::ColumbusColonize` is a POLITICAL action (`legal::
        // politics_moves`, not `action_moves`), same as `RemoveLeaderYellow`.
        r.state.phase = Phase::Politics;
        r.state.players[0].leader = CardId::by_name("Christopher Columbus").expect("Christopher Columbus is in the table");
        let territory = CardId::by_name("Vast Territory (I)").expect("Vast Territory (I) is in the table");

        // The fictional per-round deal did not happen to include this
        // specific territory, so the move is illegal until grounded.
        assert!(!r.state.players[0].hand_military.contains(territory), "test setup: must start ungrounded");
        assert!(matches!(
            r.try_apply(Move::ColumbusColonize { card: territory }, true),
            Err(MismatchKind::IllegalMove { .. })
        ));

        r.ground_for_consumption(0, territory);

        assert!(r.try_apply(Move::ColumbusColonize { card: territory }, true).is_ok());
        assert!(r.state.players[0].colonies.contains(territory));
        assert!(r.state.players[0].leader.is_none(), "Columbus is spent removing himself to colonize for free");
    }

    /// Companion to the enumerated test below: with NO filler in hand at
    /// all (an edge case `new_game` itself cannot actually produce, but a
    /// safe one to pin), there is nothing to sacrifice -- the old net-zero
    /// behaviour is left alone rather than underflowing or panicking.
    #[test]
    fn consuming_a_named_military_card_with_no_filler_in_hand_leaves_the_old_net_zero_behaviour_alone() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2;
        r.state.players[0].military_actions = 2;
        let tactic = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Tactic).expect("a Tactic card exists");
        r.state.players[0].hand_military = CardList::new();

        r.consume_named_military_card(0, tactic, Move::PlayTactic { card: tactic }, true).expect("legal once grounded");

        assert_eq!(r.state.players[0].hand_military.len(), 0, "nothing to pop -- the wash is a wash, not an underflow");
    }

    /// FIX (found chasing the corpus's dominant `hand_military_excess`
    /// drift -- 481/1011 games at delta +1, `docs/REPLAY.md`'s
    /// "Discard-phase hand-size oracle" section): the test just above pins
    /// the immediate net-zero as INTENTIONAL, but before this fix that
    /// net-zero was PERMANENT -- no debt was ever recorded, so unlike
    /// `resolve_political_decision`'s own `PrepareEvent` wash (see
    /// `preparing_an_event_with_no_disposable_filler_records_a_deficit_
    /// that_a_later_draw_repays`, above) a later real draw just stacked a
    /// phantom card on top instead of repaying anything. Mirrors that test
    /// exactly, for `consume_named_military_card`'s own no-filler path.
    #[test]
    fn consuming_a_named_military_card_with_no_disposable_filler_records_a_deficit_that_a_later_draw_repays() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2;
        r.state.players[0].military_actions = 2;
        let tactic = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Tactic).expect("a Tactic card exists");
        r.state.players[0].hand_military = CardList::new();

        r.consume_named_military_card(0, tactic, Move::PlayTactic { card: tactic }, true).expect("legal once grounded");

        assert_eq!(
            r.state.players[0].hand_military.len(),
            0,
            "no victim to sacrifice, and the played card itself leaves the hand again once `Move::PlayTactic` \
             applies -- ends back at 0, the same net-zero as before this fix"
        );
        assert_eq!(
            r.military_hand_deficit[0], 1,
            "the dropped decrement must be recorded as an owed debt, not silently lost"
        );

        // A later real draw of two cards arrives (mirrors `try_apply`'s own
        // per-seat growth detection around `apply::apply`).
        let drawn_a = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::War).expect("a War card exists");
        let drawn_b = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Pact).expect("a Pact card exists");
        r.state.players[0].hand_military.push(drawn_a);
        r.state.players[0].hand_military.push(drawn_b);
        r.repay_military_hand_deficit(0, 2);

        assert_eq!(
            r.state.players[0].hand_military.len(),
            1,
            "two cards drawn, one immediately repays the earlier wash: net +1, not +2"
        );
        assert_eq!(r.military_hand_deficit[0], 0, "the debt is now fully repaid");
    }

    /// STRUCTURAL FIX (the same "same rule fixed in one place" audit that
    /// found `resolve_political_decision`'s `PrepareEvent` net-zero wash,
    /// `docs/REPLAY.md` -- see that section): every `ActionClass` that
    /// reveals-and-plays a named military card had this EXACT shape,
    /// independently, at each of its own call sites -- `PlayTactic`/
    /// `DeclareWar`/`PlayAggression`/`ProposePact`, plus `ColumbusColonize`.
    /// Rather than pinning each site with its own near-duplicate test (the
    /// shape that let the original bug sit un-audited in three siblings
    /// after the first was fixed), this test is driven off `ActionClass`
    /// itself: `action_class_grounds_and_consumes_a_card` is an EXHAUSTIVE
    /// match with no wildcard arm, so a new `ActionClass` variant fails to
    /// compile there until someone decides whether it belongs on this list
    /// -- the mechanism that would have caught the original four-arm bug
    /// the day the second, third and fourth arm were written, instead of
    /// three separate follow-up passes later.
    ///
    /// For every variant classified `true`, this test proves the actual
    /// invariant end to end: revealing-and-playing a card the simulated
    /// hand does NOT already contain, with a real filler available to
    /// sacrifice, must decrease `hand_military.len()` by exactly one -- not
    /// zero (the wash) and not two (a double-charge).
    #[test]
    fn every_card_consuming_action_class_nets_hand_military_down_by_exactly_one() {
        fn some_card(kind: CardType) -> CardId {
            (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == kind).unwrap_or_else(|| panic!("no {kind:?} card in the table"))
        }
        // A plain Aggression card -- neither "Annex" (needs a target with a
        // colony) nor "Infiltrate" (needs a target with a leader or an
        // unfinished wonder), so `aggression_target_qualifies` accepts a
        // bare freshly-dealt opponent (`legal::aggression_target_qualifies`).
        fn plain_aggression_card() -> CardId {
            (0..crate::CARDS.len() as u16)
                .map(CardId)
                .find(|id| {
                    id.kind() == CardType::Aggression
                        && !id.get().special.iter().any(|sp| {
                            matches!(sp, crate::cards::Special::StealColony(n) if *n != 0) || matches!(sp, crate::cards::Special::RemoveFromGame)
                        })
                })
                .expect("a plain (non-Annex, non-Infiltrate) Aggression card exists")
        }
        fn fresh_replayer<'a>(card_index: &'a HashMap<&'static str, CardId>, players: u8) -> Replayer<'a> {
            Replayer::new(card_index, players, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new())
        }
        // Two SIMULATED fillers of unknown identity, standing in for
        // whatever `new_game` actually dealt -- pushed fresh per scenario so
        // no scenario's own named card collides with one of them.
        fn seed_fillers(r: &mut Replayer, actor: u8, avoid: CardId) {
            r.state.players[actor as usize].hand_military = CardList::new();
            for kind in [CardType::Aggression, CardType::War, CardType::Pact, CardType::Tactic] {
                let filler = some_card(kind);
                if filler != avoid {
                    r.state.players[actor as usize].hand_military.push(filler);
                    if r.state.players[actor as usize].hand_military.len() >= 2 {
                        break;
                    }
                }
            }
        }

        let card_index = build_card_index();
        for &class in ActionClass::ALL {
            if !action_class_grounds_and_consumes_a_card(class) {
                continue;
            }
            match class {
                ActionClass::PlayTactic => {
                    let mut r = fresh_replayer(&card_index, 2);
                    r.state.phase = Phase::Actions;
                    r.state.round = 2; // round 1 legally offers only Take/EndTurn (§1.9)
                    r.state.players[0].military_actions = 2;
                    let tactic = some_card(CardType::Tactic);
                    seed_fillers(&mut r, 0, tactic);
                    let before = r.state.players[0].hand_military.len();
                    assert!(!r.state.players[0].hand_military.contains(tactic), "{class:?}: test setup must start ungrounded");

                    r.consume_named_military_card(0, tactic, Move::PlayTactic { card: tactic }, true).expect("legal once grounded");

                    assert_eq!(r.state.players[0].hand_military.len(), before - 1, "{class:?}: must net -1, not 0");
                }
                ActionClass::DeclareWar => {
                    let mut r = fresh_replayer(&card_index, 2);
                    r.state.phase = Phase::Politics;
                    let war = some_card(CardType::War);
                    r.state.players[0].military_actions = war.get().military_action_cost as i8 + 2;
                    seed_fillers(&mut r, 0, war);
                    let before = r.state.players[0].hand_military.len();
                    assert!(!r.state.players[0].hand_military.contains(war), "{class:?}: test setup must start ungrounded");

                    r.consume_named_military_card(0, war, Move::War { card: war, target: 1 }, true).expect("legal once grounded");

                    assert_eq!(r.state.players[0].hand_military.len(), before - 1, "{class:?}: must net -1, not 0");
                }
                ActionClass::PlayAggression => {
                    let mut r = fresh_replayer(&card_index, 2);
                    r.state.phase = Phase::Politics;
                    let agg = plain_aggression_card();
                    r.state.players[0].military_actions = agg.get().military_action_cost as i8 + 2;
                    // Attacker strictly stronger, defender no strength at
                    // all -- `politics_aggression_generated_when_attacker_
                    // is_strictly_stronger`'s own recipe (`legal.rs`).
                    r.state.players[0].techs = crate::state::Tableau::new();
                    r.state.players[0].techs.insert(CardId::by_name("Warriors").expect("Warriors is in the table"), crate::state::TechSlot { workers: 3, stored: 0 });
                    r.state.players[1].techs = crate::state::Tableau::new();
                    seed_fillers(&mut r, 0, agg);
                    let before = r.state.players[0].hand_military.len();
                    assert!(!r.state.players[0].hand_military.contains(agg), "{class:?}: test setup must start ungrounded");

                    r.consume_named_military_card(0, agg, Move::Aggression { card: agg, target: 1 }, true).expect("legal once grounded");

                    assert_eq!(r.state.players[0].hand_military.len(), before - 1, "{class:?}: must net -1, not 0");
                }
                ActionClass::ProposePact => {
                    // §13 / CoL p.2: pacts are removed from the military
                    // decks entirely at 2p (`legal::politics_offer_pact_not_
                    // generated_at_two_players`) -- needs 3.
                    let mut r = fresh_replayer(&card_index, 3);
                    r.state.phase = Phase::Politics;
                    let pact = CardId::by_name("Peace Treaty").expect("Peace Treaty is in the table");
                    seed_fillers(&mut r, 0, pact);
                    let before = r.state.players[0].hand_military.len();
                    assert!(!r.state.players[0].hand_military.contains(pact), "{class:?}: test setup must start ungrounded");

                    r.consume_named_military_card(0, pact, Move::OfferPact { card: pact, target: 1, side: PactSide::Unspecified }, true)
                        .expect("legal once grounded");

                    assert_eq!(r.state.players[0].hand_military.len(), before - 1, "{class:?}: must net -1, not 0");
                }
                ActionClass::ColumbusColonize => {
                    // Cannot use `consume_named_military_card` (an
                    // unavoidable `resolve_intervening` step sits between
                    // the ground and the consuming `Move` at this call site
                    // -- see `ground_for_consumption`'s own doc) -- exercise
                    // the documented low-level exception directly instead.
                    let mut r = fresh_replayer(&card_index, 2);
                    r.state.phase = Phase::Politics;
                    r.state.players[0].leader = CardId::by_name("Christopher Columbus").expect("in the table");
                    let territory = CardId::by_name("Vast Territory (I)").expect("in the table");
                    seed_fillers(&mut r, 0, territory);
                    let before = r.state.players[0].hand_military.len();
                    assert!(!r.state.players[0].hand_military.contains(territory), "{class:?}: test setup must start ungrounded");

                    r.ground_for_consumption(0, territory);
                    r.try_apply(Move::ColumbusColonize { card: territory }, true).expect("legal once grounded");

                    assert_eq!(r.state.players[0].hand_military.len(), before - 1, "{class:?}: must net -1, not 0");
                }
                other @ ActionClass::TakeCard | other @ ActionClass::BuildBuilding | other @ ActionClass::BuildUnit | other @ ActionClass::BuildWonderStage | other @ ActionClass::IncreasePopulation | other @ ActionClass::UpgradeUnit | other @ ActionClass::UpgradeProduction | other @ ActionClass::DevelopTechnology | other @ ActionClass::ElectLeader | other @ ActionClass::ChangeGovernment | other @ ActionClass::WinWar | other @ ActionClass::AcceptPact | other @ ActionClass::Colonize | other @ ActionClass::Discard | other @ ActionClass::Bid | other @ ActionClass::WinAuction | other @ ActionClass::Destroy | other @ ActionClass::Disband | other @ ActionClass::Pass | other @ ActionClass::PlayEvent | other @ ActionClass::PlayActionCard | other @ ActionClass::PutBack | other @ ActionClass::EndTurn | other @ ActionClass::RemoveLeaderYellow | other @ ActionClass::Barbarossa | other @ ActionClass::BachTheater => unreachable!(
                    "{other:?} is classified as card-consuming by action_class_grounds_and_consumes_a_card                      but this test has no scenario for it -- add one alongside the classification"
                ),
            }
        }
    }

    /// FIX (same audit, same shape, a third call site): `ground_auction_
    /// winner_hand` grounds a bonus card the journal says the winner is
    /// about to sacrifice -- and that grounding is consumed for real, just
    /// later, once `drain_colonize`'s own `Move::SendBonus` removes the
    /// identical identity (`interact::auction_move`). A real auction winner
    /// who sacrifices a bonus card they never revealed beforehand already
    /// HELD it: their hand shrinks by one per card sacrificed, not zero.
    #[test]
    fn sacrificing_a_grounded_auction_bonus_card_shrinks_the_hand_by_one_not_zero() {
        let card_index = build_card_index();
        let warriors = card_index["Warriors"];
        let bonus3 = colonization_bonus_card(3).unwrap();
        let territory = card_index["Vast Territory (I)"];
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 107,
            actor: 1,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(warriors), SacrificeClause::Bonus(bonus3)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        // Two SIMULATED fillers of unknown identity -- neither is `bonus3`,
        // the card the journal is about to name.
        let filler_a = colonization_bonus_card(2).unwrap();
        let filler_b = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Tactic).expect("a Tactic card exists");
        r.state.players[1].hand_military = CardList::new();
        r.state.players[1].hand_military.push(filler_a);
        r.state.players[1].hand_military.push(filler_b);
        r.state.players[1].techs.get_mut(warriors).expect("every player starts with Warriors").workers = 2;
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[1, 0], 1, 3, Some(1), 0)));

        r.ground_auction_winner_hand();
        assert_eq!(
            r.state.players[1].hand_military.len(),
            2,
            "grounding a phantom bonus card pops one filler first: 2 fillers -> 1 filler + the named \
             card, still 2 total, not 3 -- hand={:?}",
            r.state.players[1].hand_military.as_slice()
        );
        assert!(r.state.players[1].hand_military.contains(bonus3));

        crate::interact::colonize(&mut r.state, 1, territory, 4);
        r.drain_colonize().expect("the journal's own list is legal here");

        assert_eq!(
            r.state.players[1].hand_military.len(),
            1,
            "sacrificing the (previously phantom) bonus card must net -1 against the pre-auction hand, not 0"
        );
    }

    /// FIX (same audit, same shape, a third call site -- see `ground_for_
    /// consumption`'s own `consuming_a_named_military_card_with_no_
    /// disposable_filler_records_a_deficit_that_a_later_draw_repays`,
    /// above, which this mirrors exactly): the test just above pins the
    /// ordinary case, where a disposable filler exists to evict. When NO
    /// filler exists -- here, the winner's hand holds nothing at all, the
    /// simplest instance of "nothing evictable" -- `ground_auction_winner_
    /// hand` used to `push` the phantom bonus card anyway with no
    /// compensating eviction and no debt recorded anywhere, so `drain_
    /// colonize`'s own `Move::SendBonus` right after nets the pair to zero
    /// PERMANENTLY instead of the real -1 a genuine sacrifice leaves
    /// behind.
    #[test]
    fn sacrificing_a_grounded_auction_bonus_card_with_no_disposable_filler_records_a_deficit_that_a_later_draw_repays()
    {
        let card_index = build_card_index();
        let warriors = card_index["Warriors"];
        let bonus3 = colonization_bonus_card(3).unwrap();
        let territory = card_index["Vast Territory (I)"];
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 107,
            actor: 1,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(warriors), SacrificeClause::Bonus(bonus3)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        // No filler at all -- the emptiest possible "nothing to evict" hand.
        r.state.players[1].hand_military = CardList::new();
        r.state.players[1].techs.get_mut(warriors).expect("every player starts with Warriors").workers = 2;
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[1, 0], 1, 3, Some(1), 0)));

        r.ground_auction_winner_hand();
        assert_eq!(
            r.state.players[1].hand_military.len(),
            1,
            "no filler to evict, so the phantom bonus card just lands: 0 -> 1, not the usual net-zero 0 -> 0"
        );
        assert!(r.state.players[1].hand_military.contains(bonus3));
        assert_eq!(
            r.military_hand_deficit[1], 1,
            "the dropped eviction must be recorded as an owed debt, not silently lost"
        );

        crate::interact::colonize(&mut r.state, 1, territory, 4);
        r.drain_colonize().expect("the journal's own list is legal here");
        assert_eq!(
            r.state.players[1].hand_military.len(),
            0,
            "sacrificing the (previously phantom) bonus card nets the pair to zero, same as before this fix -- \
             the fix is that a debt now exists to be repaid, not that this net changes"
        );
        assert_eq!(
            r.military_hand_deficit[1], 1,
            "drain_colonize does not itself repay the debt -- only a later genuine draw does"
        );

        // A later real draw of two cards arrives (mirrors `try_apply`'s own
        // per-seat growth detection around `apply::apply`).
        let drawn_a = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::War).expect("a War card exists");
        let drawn_b = (0..crate::CARDS.len() as u16).map(CardId).find(|id| id.kind() == CardType::Pact).expect("a Pact card exists");
        r.state.players[1].hand_military.push(drawn_a);
        r.state.players[1].hand_military.push(drawn_b);
        r.repay_military_hand_deficit(1, 2);

        assert_eq!(
            r.state.players[1].hand_military.len(),
            1,
            "two cards drawn, one immediately repays the earlier wash: net +1, not +2"
        );
        assert_eq!(r.military_hand_deficit[1], 0, "the debt is now fully repaid");
    }

    /// REGRESSION (found chasing the Build/Upgrade/WonderStep cost-mismatch
    /// cluster, 135 games / 425 lines corpus-wide): `"Barbarossa enlists a
    /// <Unit>; ..."` used to classify as `Bookkeeping` (`corpus.rs`) and was
    /// never applied at all, silently dropping both the free population
    /// increase and the unit build -- drifting `yellow_bank`/`workers_free`/
    /// `resources` for the rest of the game. Values mirror
    /// `apply::tests::h_barbarossa_grows_population_and_builds_a_unit_for_one_military_action`,
    /// the direct-engine-call test this dispatch now routes to for real.
    #[test]
    fn a_barbarossa_enlist_line_applies_the_free_pop_increase_and_the_unit_build() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2; // round 1 legally offers only `Take`/`EndTurn` (§1.9)
        let warriors = CardId::by_name("Warriors").expect("Warriors is in the table");
        {
            let p = &mut r.state.players[0];
            p.leader = CardId::by_name("Frederick Barbarossa").expect("in the table");
            p.military_actions = 2;
            p.civil_actions = 4;
            p.yellow_bank = 14; // pop_cost_base(14) == 3, minus his 1 food discount == 2
            p.food = 2;
            p.resources = 1; // Warriors costs 2, minus his 1 resource discount == 1
            // Warriors is a `START_TECHS` entry with 1 worker already --
            // zero it so this is a fresh build, matching the journal text.
            p.techs.get_mut(warriors).expect("a starting tech").workers = 0;
        }

        // Exercises the real dispatch this bug lived in: `try_apply` is what
        // both the special no-leading-colour loop branch AND `apply_one`'s
        // (unreachable, for this class) own arm ultimately bottom out on --
        // `classify`'s own resolution into `ActionClass::Barbarossa` is
        // covered separately in `corpus.rs`'s tests.
        let out = r.try_apply(Move::Barbarossa { card: warriors }, true);

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert_eq!(p.techs.workers(warriors), 1, "the new worker built the unit");
        assert_eq!(p.yellow_bank, 13, "one token left the bank for the free pop increase");
        assert_eq!(p.food, 0, "paid the discounted population cost");
        assert_eq!(p.resources, 0, "paid the discounted build cost");
        assert_eq!(p.military_actions, 1, "the ONE military action bought both halves");
    }

    /// REGRESSION (found chasing the same cluster, 79 games / 111 lines
    /// corpus-wide): `"Johannes Sebastian Bachupgrades <From> to <To> ..."`
    /// used to classify as `Bookkeeping` and was never applied, silently
    /// dropping the resource spend and the tableau change. Values mirror
    /// `apply::tests::h_bach_theater_upgrades_cross_type_pays_the_difference_and_marks_used_once`.
    #[test]
    fn a_bach_upgrade_line_applies_the_cross_family_theater_conversion() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2; // round 1 legally offers only `Take`/`EndTurn` (§1.9)
        let theology = CardId::by_name("Theology").expect("in the table");
        let drama = CardId::by_name("Drama").expect("in the table");
        {
            let p = &mut r.state.players[0];
            p.leader = CardId::by_name("J. S. Bach").expect("in the table");
            p.civil_actions = 4;
            p.resources = 10;
            p.techs.insert(theology, crate::state::TechSlot { workers: 1, stored: 0 });
            p.techs.insert(drama, crate::state::TechSlot { workers: 0, stored: 0 });
        }

        let raw = "Johannes Sebastian Bachupgrades Theology to Drama Orange spends 1 resource";
        let out = apply_one(&mut r, 0, ActionClass::BachTheater, Some(drama), "upgrades Theology to Drama Orange spends 1 resource", raw, None);

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert!(p.bach_upgrade_used, "at most once per turn");
        assert_eq!(p.civil_actions, 3);
        assert_eq!(p.techs.workers(theology), 0);
        assert_eq!(p.techs.workers(drama), 1);
    }

    /// REGRESSION (found chasing the Build/Upgrade/WonderStep "resources
    /// short by a small amount" cluster, real game `7521776`): a `"takes
    /// Patriotism in hand"` line earlier in the same game resolves the card
    /// age-blind (`best_age_sibling`, gated only on `age_civil`) and can
    /// land the WRONG age-sibling in `hand_civil` -- here, the Age I copy
    /// (`resourcesForMilitaryUnits: 2`) when the row/deck actually dealt the
    /// Age A copy (`resourcesForMilitaryUnits: 1`). Before this fix, `solved`
    /// stayed `None` for Patriotism (it has no `FreeCivilAction` special, so
    /// the `kind` match never even tried), so the fallback trusted the wrong
    /// hand entry and credited `mil_discount` with DOUBLE the real bonus --
    /// which then silently overpaid a same-turn unit build by 1 fewer
    /// resource than the human actually spent, a shortfall that compounds
    /// turn over turn into a much-later `IllegalMove: Upgrade` (this exact
    /// game, round 8: `Upgrade { from: Warriors, to: Swordsmen }` rejected
    /// with `resources=0` when the journal's own arithmetic implies 1).
    #[test]
    fn a_patriotism_play_line_resolves_the_age_sibling_from_its_own_bonus_clause_not_the_earlier_take_guess() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2; // round 1 legally offers only `Take`/`EndTurn` (§1.9)
        let patriotism_a = CardId::by_name("Patriotism (A)").expect("in the table");
        let patriotism_i = CardId::by_name("Patriotism (I)").expect("in the table");
        {
            let p = &mut r.state.players[0];
            p.civil_actions = 4;
            p.hand_civil = CardList::new();
            // The earlier take-time guess put the WRONG age-sibling in hand.
            p.hand_civil.push(patriotism_i);
        }

        let raw = "Orange plays Patriotism Orange gets 1 military resource; Orange gets 1 military action";
        let out = apply_one(&mut r, 0, ActionClass::PlayActionCard, Some(patriotism_a), "plays Patriotism Orange gets 1 military resource; Orange gets 1 military action", raw, None);

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert_eq!(p.mil_discount, 1, "Age A's printed bonus, not Age I's double-counted one");
        // `h_play_action` removes the played card from hand as part of
        // playing it -- so the evidence-backed correction shows up as the
        // WRONG guess being gone, not as the right one still sitting there.
        assert!(!p.hand_civil.contains(patriotism_i), "the wrong age guess was corrected, not played as-is");
        assert!(!p.hand_civil.contains(patriotism_a), "the corrected card was then played (removed), not left in hand");
    }

    /// REGRESSION, same shape as the Patriotism one above (found chasing
    /// the same cluster): Reserves' `Special::GainFoodOrResources` also
    /// scales by age (2/3/4 for I/II/III) with no `FreeCivilAction` special
    /// to route through the `kind` match, and its own trailing `"produces N
    /// food/resources"` clause -- already parsed for the food-vs-resources
    /// CHOICE below -- was going unused for the CARD identity, so a wrong
    /// take-time age guess granted the wrong-age amount.
    #[test]
    fn a_reserves_play_line_resolves_the_age_sibling_from_its_own_gain_clause_not_the_earlier_take_guess() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2; // round 1 legally offers only `Take`/`EndTurn` (§1.9)
        let reserves_ii = CardId::by_name("Reserves (II)").expect("in the table");
        let reserves_iii = CardId::by_name("Reserves (III)").expect("in the table");
        {
            let p = &mut r.state.players[0];
            p.civil_actions = 4;
            p.hand_civil = CardList::new();
            // The earlier take-time guess put the WRONG age-sibling in hand
            // (Age III, gainFoodOrResources 4) when the real card is Age II
            // (gainFoodOrResources 3, matching the line's own "produces 3
            // resources" evidence below).
            p.hand_civil.push(reserves_iii);
        }

        let raw = "Orange plays Reserves Orange produces 3 resources";
        let out = apply_one(&mut r, 0, ActionClass::PlayActionCard, Some(reserves_ii), "plays Reserves Orange produces 3 resources", raw, None);

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert_eq!(p.resources, 3, "Age II's printed gain (3), not Age III's wrong one (4)");
        assert!(!p.hand_civil.contains(reserves_iii), "the wrong age guess was corrected, not played as-is");
        assert!(!p.hand_civil.contains(reserves_ii), "the corrected card was then played (removed), not left in hand");
    }

    /// REGRESSION, same shape as the Patriotism/Reserves tests above --
    /// this pass's own task: `docs/REPLAY.md`'s Build/Upgrade/WonderStep
    /// handoff explicitly named Cultural Heritage and Revolutionary Idea as
    /// the two remaining age-scaled action-card families never checked.
    /// Cultural Heritage has no `Special` at all (`gainScience`/
    /// `gainCulture` only), so nothing routes it through the `kind` match,
    /// and a wrong take-time age guess would silently apply the WRONG age's
    /// science/culture gain.
    #[test]
    fn a_cultural_heritage_play_line_resolves_the_age_sibling_from_its_own_science_clause_not_the_earlier_take_guess() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2; // round 1 legally offers only `Take`/`EndTurn` (§1.9)
        let heritage_a = CardId::by_name("Cultural Heritage (A)").expect("in the table");
        let heritage_i = CardId::by_name("Cultural Heritage (I)").expect("in the table");
        {
            let p = &mut r.state.players[0];
            p.civil_actions = 4;
            p.hand_civil = CardList::new();
            // The earlier take-time guess put the WRONG age-sibling in hand
            // (Age I: gainScience 2, gainCulture 2) when the real card is
            // Age A (gainScience 1, gainCulture 4, matching the line's own
            // "gets 1 science; scores 4 culture" evidence below).
            p.hand_civil.push(heritage_i);
        }

        let raw = "Orange plays Cultural Heritage Orange gets 1 science; Orange scores 4 culture";
        let out = apply_one(&mut r, 0, ActionClass::PlayActionCard, Some(heritage_a), "plays Cultural Heritage Orange gets 1 science; Orange scores 4 culture", raw, None);

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert_eq!(p.science, 1, "Age A's printed science gain (1), not Age I's wrong one (2)");
        assert_eq!(p.culture, 4, "Age A's printed culture gain (4), not Age I's wrong one (2)");
        assert!(!p.hand_civil.contains(heritage_i), "the wrong age guess was corrected, not played as-is");
        assert!(!p.hand_civil.contains(heritage_a), "the corrected card was then played (removed), not left in hand");
    }

    /// Same shape, Revolutionary Idea's side: no culture clause at all
    /// (`gainScience` only, 4 vs 6 for age II/III), pinning that
    /// `trailing_gets_science`'s reuse here does not depend on a
    /// following "scores" clause being present.
    #[test]
    fn a_revolutionary_idea_play_line_resolves_the_age_sibling_from_its_own_science_clause_not_the_earlier_take_guess() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2;
        let idea_ii = CardId::by_name("Revolutionary Idea (II)").expect("in the table");
        let idea_iii = CardId::by_name("Revolutionary Idea (III)").expect("in the table");
        {
            let p = &mut r.state.players[0];
            p.civil_actions = 4;
            p.hand_civil = CardList::new();
            // The earlier take-time guess put the WRONG age-sibling in hand
            // (Age III, gainScience 6) when the real card is Age II
            // (gainScience 4, matching the line's own "gets 4 science"
            // evidence below).
            p.hand_civil.push(idea_iii);
        }

        let raw = "Orange plays Revolutionary Idea Orange gets 4 science";
        let out = apply_one(&mut r, 0, ActionClass::PlayActionCard, Some(idea_ii), "plays Revolutionary Idea Orange gets 4 science", raw, None);

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert_eq!(p.science, 4, "Age II's printed gain (4), not Age III's wrong one (6)");
        assert!(!p.hand_civil.contains(idea_iii), "the wrong age guess was corrected, not played as-is");
        assert!(!p.hand_civil.contains(idea_ii), "the corrected card was then played (removed), not left in hand");
    }

    /// This pass's fix 2 (`ActionClass::PlayActionCard`'s `FreeActionKind::
    /// IncreasePopulation` arm, right before `r.try_apply(Move::PlayAction
    /// { card }, true)`): mirrors the synthesis the ordinary `ActionClass::
    /// IncreasePopulation` arm already makes (its own doc comment, a few
    /// hundred lines above) for THIS "plays Frugality" line. A live Trade
    /// Routes Agreement side-B grant (§5.9, "Civilization B can use 1
    /// resource as 1 food during its turn") lets Frugality's ordered Pop be
    /// paid PART in converted resources, and BGO glues that as a second
    /// "<Color> spends M resource" clause onto the SAME line, with no
    /// preceding `Move::TradeResourceAsFood` anywhere in the journal.
    /// Before this fix this arm never tried the conversion at all: with
    /// `p.food` short of `pop_cost` by exactly the pact's grant, `Move::Pop`
    /// was never in `free_action_moves`, `legal::action_card_playable` read
    /// false, and `Move::PlayAction { card: Frugality }` was never even
    /// offered as legal -- an `IllegalMove: PlayAction` for a line the human
    /// actually played. Found chasing that bucket (games 7522213, 7522672).
    #[test]
    fn a_frugality_play_line_short_on_food_converts_the_shortfall_via_a_live_trade_routes_agreement_grant() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.round = 2; // round 1 legally offers only `Take`/`EndTurn` (§1.9)
        let frugality_a = CardId::by_name("Frugality (A)").expect("in the table");
        {
            let p = &mut r.state.players[0];
            p.civil_actions = 1;
            p.yellow_bank = 5; // pop_cost_base(5) == 5
            p.food = 4; // short by 1 -- exactly the pact's own grant magnitude
            p.resources = 1;
            p.blue_total = 20; // room for both the conversion's +1 food and Frugality's own +1 food gain
            p.hand_civil = CardList::new();
            p.hand_civil.push(frugality_a);
            // Trade Routes Agreement, side B: player 0 may use 1 resource as
            // 1 food this turn (`give_trade_routes_side_a`'s own doc comment
            // a little below has the sibling side-A helper for the Build/
            // Upgrade/WonderStep path; `a`/`b` are the real party-to-block
            // mapping, not "owner is A" -- `state.rs`'s own doc on `Pact`).
            p.pacts.push(crate::state::Pact {
                card: CardId::by_name("Trade Routes Agreement").expect("Trade Routes Agreement is a known card"),
                owner: 0,
                partner: 1,
                a: 1,
                b: 0,
            });
        }

        let raw = "Orange plays Frugality Orange increases population Orange spends 4 food; Orange spends 1 resource Orange produces 1 food";
        let out = apply_one(
            &mut r,
            0,
            ActionClass::PlayActionCard,
            Some(frugality_a),
            "plays Frugality Orange increases population Orange spends 4 food; Orange spends 1 resource Orange produces 1 food",
            raw,
            None,
        );

        assert!(out.is_ok(), "{out:?}");
        let p = &r.state.players[0];
        assert_eq!(p.resources, 0, "the shortfall resource was converted, not left unspent");
        assert_eq!(p.trade_resource_as_food_used_this_turn, 1, "via a real Move::TradeResourceAsFood, not a silent discount");
        assert!(!p.hand_civil.contains(frugality_a), "Frugality was actually played");
        assert_eq!(p.food, 1, "5 (post-conversion) - 5 (pop cost) + 1 (Frugality's own gainFood)");
        assert_eq!(p.yellow_bank, 4, "the population actually increased (one cheap token spent from the bank)");
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
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
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

    /// The whole point of the sacrifice record: BGO's colonize line is one
    /// clause PER committed piece, not a bare force total. Real line from
    /// game `7523818`, plus a second one carrying every other clause shape
    /// the corpus contains (a repeated bonus card, a James Cook discard, and
    /// the `"Colonization bonus:"` / `"Total force:"` / reward bookkeeping
    /// clauses that name no card at all and must be skipped, not guessed at).
    #[test]
    fn a_colonize_line_names_every_sacrificed_unit_and_bonus_card_individually() {
        let card_index = build_card_index();
        let (territory, clauses) = parse_sacrifice_clauses(
            "Purple colonizes a Vast Territory Sacrificed Units:; 1 Warrior; 1 Warrior; \
             1 Colonization card +1; Total force: 4; Purple produces 3 food",
            &card_index,
        )
        .expect("a colonize line parses");
        assert_eq!(territory, "Vast Territory", "the age suffix is never printed");
        assert_eq!(
            clauses,
            vec![
                SacrificeClause::Unit(card_index["Warriors"]),
                SacrificeClause::Unit(card_index["Warriors"]),
                SacrificeClause::Bonus(colonization_bonus_card(1).unwrap()),
            ]
        );

        let (_, clauses) = parse_sacrifice_clauses(
            "Green colonizes a Strategic Territory Sacrificed Units:; 1 Knights; \
             1 Colonization card +2; 1 Colonization card +2; 1 Military card +1; \
             Colonization bonus: +2; Total force: 9; Green gets 2 population",
            &card_index,
        )
        .expect("a colonize line parses");
        assert_eq!(
            clauses,
            vec![
                SacrificeClause::Unit(card_index["Knights"]),
                SacrificeClause::Bonus(colonization_bonus_card(2).unwrap()),
                SacrificeClause::Bonus(colonization_bonus_card(2).unwrap()),
                // James Cook's discard is the ONE piece whose identity the
                // journal really does withhold -- only its count is claimed.
                SacrificeClause::CookDiscard,
            ]
        );

        assert!(parse_sacrifice_clauses("Purple bids 3", &card_index).is_none(), "not a colonize line");
    }

    /// REGRESSION (real game `7522267`): every other unit card BGO's journal
    /// names is one word ("Warrior"/"Cannon"/"Knights"/...), but "Modern
    /// Infantry" (age IV) is two, and the old `[unit]` single-word match arm
    /// silently swallowed its clause into the same `_ => None` catch-all
    /// that legitimately skips `"Colonization bonus: +N"` / `"Total force:
    /// N"` -- indistinguishable, from the caller's side, from the human not
    /// having sacrificed a Modern Infantry at all. That dropped clause left
    /// `drain_colonize` a unit short of what the journal's own `"Total
    /// force"` demanded, which it could never make up (every other unit was
    /// already spoken for), forcing `approximate_colonize` and, on the real
    /// game, a `StuckPending` a full colonize event later once the
    /// mis-tracked army ran out of substitutes entirely.
    #[test]
    fn a_colonize_line_names_a_two_word_unit_card_and_does_not_drop_it() {
        let card_index = build_card_index();
        let (_, clauses) = parse_sacrifice_clauses(
            "Purple colonizes a Developed Territory Sacrificed Units:; 1 Cannon; 1 Warrior; \
             1 Modern Infantry; Colonization bonus: +2; Total force: 11; Purple gets 5 science",
            &card_index,
        )
        .expect("a colonize line parses");
        assert_eq!(
            clauses,
            vec![
                SacrificeClause::Unit(card_index["Cannon"]),
                SacrificeClause::Unit(card_index["Warriors"]),
                SacrificeClause::Unit(card_index["Modern Infantry"]),
            ],
            "the two-word unit name must not be silently dropped"
        );
    }

    /// Each of the three military bonus cards prints a distinct colonization
    /// value, so `"+N"` alone is a full card identity -- the same property
    /// `defense_bonus_card` relies on, asserted here so a future card-table
    /// edit that broke it could not pass silently.
    #[test]
    fn each_printed_colonization_bonus_value_identifies_exactly_one_card() {
        for value in 1..=3 {
            let matching: Vec<CardId> = (0..crate::CARDS.len() as u16)
                .map(CardId)
                .filter(|id| id.kind() == CardType::Bonus && id.get().effects.colonization_bonus == value)
                .collect();
            assert_eq!(matching.len(), 1, "colonization bonus +{value} must name exactly one card");
            assert_eq!(colonization_bonus_card(value), Some(matching[0]));
        }
        assert_eq!(colonization_bonus_card(4), None, "the base game prints no +4 bonus card");
    }

    /// REGRESSION: the journal's own sacrifice list decides which units die,
    /// not `interact::colonize_moves`'s weakest-first fallback ordering.
    ///
    /// The human here spent their KNIGHT and a +2 bonus card, keeping their
    /// Warriors. The old auto-drain took the engine's first offered move at
    /// every step, which is the weakest unit first -- it would have killed
    /// the Warriors AND (still short of the bid) the Knights, permanently
    /// removing an army token the human never spent and quietly lowering
    /// every colonization ceiling and military strength that player had for
    /// the rest of the game.
    #[test]
    fn a_colonization_sacrifices_exactly_the_units_the_journal_names_and_no_others() {
        let card_index = build_card_index();
        let warriors = card_index["Warriors"];
        let knights = card_index["Knights"];
        let bonus2 = colonization_bonus_card(2).unwrap();
        let territory = card_index["Vast Territory (I)"];
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 107,
            actor: 0,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(knights), SacrificeClause::Bonus(bonus2)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        // `new_game` already deals every player a Warriors tableau card.
        r.state.players[0].techs.get_mut(warriors).expect("every player starts with Warriors").workers = 2;
        r.state.players[0].techs.insert(knights, crate::state::TechSlot { workers: 1, stored: 0 });
        r.state.players[0].hand_military = CardList::new();
        r.state.players[0].hand_military.push(bonus2);

        // Knight (2) + bonus (+2) is exactly the bid; so is Warrior +
        // Warrior + bonus, which is what the weakest-first fallback picks.
        crate::interact::colonize(&mut r.state, 0, territory, 4);
        r.drain_colonize().expect("the journal's own list is legal here");

        assert!(r.state.pending.is_empty(), "the force resolved");
        assert_eq!(r.state.players[0].techs.get(warriors).map(|s| s.workers), Some(2), "the Warriors were kept");
        assert_eq!(r.state.players[0].techs.get(knights).map(|s| s.workers), Some(0), "the Knight was sacrificed");
        assert!(!r.state.players[0].hand_military.contains(bonus2), "the named bonus card left the hand");
        assert!(!r.colonize_approximated, "this colonization was replayed, not approximated");
    }

    /// REGRESSION (real games `7521757`/`7523177`, hand-traced in
    /// `analysis/worker_notes_2026-08-14/popor__POPOR.txt`): the journal
    /// names the sacrificed unit, but this binary's own reconstructed ARMY
    /// (built off earlier, imperfectly-replayed `Build`/`Upgrade` actions)
    /// simply does not have that unit card in the player's tableau yet --
    /// the unit's tech card was never inserted, only a DIFFERENT unit type
    /// (`Warriors` here) is available. Before `ground_army_unit_for_
    /// consumption` existed, `drain_colonize` had no legal continuation for
    /// `Move::SendUnit { card: Knights }` and fell straight to
    /// `approximate_colonize`, which -- offering the engine's own
    /// first-offered move -- would have sacrificed the (kept, cheaper)
    /// Warriors instead, exactly the "more, weaker substitute units" failure
    /// this whole pass exists to close.
    ///
    /// The fix grounds `Knights` into the tableau by swapping ONE worker off
    /// `Warriors` onto it -- total army size unchanged, only the reconstructed
    /// TYPE composition corrected to match the journal's own claim.
    #[test]
    fn a_colonization_grounds_a_named_unit_missing_from_the_reconstructed_army_by_swapping_it_in() {
        let card_index = build_card_index();
        let warriors = card_index["Warriors"];
        // A second, DISTINCT Age I unit type, so `interact::colonize`'s own
        // auto-drain (`colonize_auto`: "keep applying while exactly one
        // legal move exists") does not just greedily consume every Warrior
        // on its own before `drain_colonize` ever runs -- with two types
        // available, `colonize_moves` always offers 2+ choices, so control
        // reaches `drain_colonize` with `Pending::Colonize` still fully
        // open, exactly like a real branching colonization.
        let swordsmen = card_index["Swordsmen"];
        let knights = card_index["Knights"];
        let territory = card_index["Vast Territory (I)"];
        let bid = knights.get().effects.strength as u8;
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 200,
            actor: 0,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(knights)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        // One Warriors, one Swordsmen, NO Knights slot at all -- this
        // binary's own reconstruction is missing the unit the journal names.
        r.state.players[0].techs.get_mut(warriors).expect("every player starts with Warriors").workers = 1;
        r.state.players[0].techs.insert(swordsmen, crate::state::TechSlot { workers: 1, stored: 0 });
        assert!(!r.state.players[0].techs.has(knights), "the test fixture starts without Knights, on purpose");

        crate::interact::colonize(&mut r.state, 0, territory, bid);
        r.drain_colonize().expect("the journal's own list is legal here once the named unit is grounded");

        assert!(r.state.pending.is_empty(), "the force resolved");
        assert!(!r.colonize_approximated, "this colonization was replayed, not approximated");
        assert_eq!(
            r.state.players[0].techs.get(knights).map(|s| s.workers),
            Some(0),
            "the Knight was grounded in, then immediately sacrificed"
        );
        let remaining_army = r.state.players[0].techs.workers(warriors) + r.state.players[0].techs.workers(swordsmen);
        assert_eq!(
            remaining_army, 1,
            "exactly one worker (Warriors or Swordsmen) was swapped out to ground the Knight -- \
             army size is unchanged (started at 2), not grown"
        );
    }

    /// REGRESSION (found chasing the culture-oracle divergence on real game
    /// `7522152`): a colonize whose `Pending::Colonize` has EXACTLY one legal
    /// continuation at every step (a single available unit, no bonus/discard
    /// cards) resolves entirely inside `interact::colonize`'s own
    /// `colonize_auto`, with `resolve_intervening` -- the only caller of
    /// `drain_colonize`, the only OTHER place that pops `colonize_sacrifices`
    /// -- never getting a chance to see the `Pending::Colonize` at all. Before
    /// this fix, that left player 0's own queue entry sitting at the front
    /// forever, so player 1's LATER, real, branching colonize found the wrong
    /// front entry (`sac.actor != player`) and silently fell back to
    /// `approximate_colonize`'s "take the engine's first-offered move" rule
    /// instead of the journal's own exact sacrifice list -- exactly the
    /// failure `a_colonization_sacrifices_exactly_the_units_the_journal_
    /// names_and_no_others` (above) pins for a queue that starts aligned.
    /// `ActionClass::Colonize`'s dispatch arm (`apply_one`, called for the
    /// `"<Color> colonizes ..."` confirmation line itself) is the one place
    /// left that ever sees a fully-forced colonize, so popping its entry
    /// there -- but ONLY when `Pending::Colonize` is already gone, never when
    /// draining is still owed to a later `resolve_intervening` iteration -- is
    /// what fixes it.
    #[test]
    fn a_fully_forced_colonize_pops_its_own_queue_entry_so_a_later_colonize_is_not_misattributed() {
        let card_index = build_card_index();
        let warriors = card_index["Warriors"];
        let knights = card_index["Knights"];
        let bonus2 = colonization_bonus_card(2).unwrap();
        let territory0 = card_index["Vast Territory (I)"];
        let territory1 = card_index["Vast Territory (II)"];
        let mut sacrifices = VecDeque::new();
        // Player 0's own entry: what the journal claims was sacrificed does
        // not matter here (the forced path never reads it), but a real line
        // always has one.
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 50,
            actor: 0,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(warriors)],
        });
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 200,
            actor: 1,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(knights), SacrificeClause::Bonus(bonus2)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        // Player 0 has exactly one sacrificeable unit (`new_game`'s starting
        // Warriors, strength 1) and an empty bonus/discard pool -- so a bid
        // of 1 leaves `colonize_moves` exactly one legal move at every step
        // (`SendUnit { Warriors }`, then `SendDone`), and `colonize_auto`
        // walks the whole thing to completion with no external driving at all.
        r.state.players[0].hand_military = CardList::new();
        crate::interact::colonize(&mut r.state, 0, territory0, 1);
        assert!(r.state.pending.is_empty(), "a single-legal-move-per-step colonize must fully auto-resolve");
        assert_eq!(r.colonize_sacrifices.len(), 2, "nothing has popped yet -- the confirmation line has not been reached");

        // Reaching player 0's own confirmation line (`ActionClass::Colonize`
        // dispatch) is what must pop its now-stale entry.
        r.current_lineno = 50;
        let out = apply_one(&mut r, 0, ActionClass::Colonize, Some(territory0), "colonizes a Vast Territory", "Orange colonizes a Vast Territory Sacrificed Units:; 1 Warrior; Total force: 1", None);
        assert!(out.is_ok(), "{out:?}");
        assert_eq!(r.colonize_sacrifices.len(), 1, "player 0's own entry popped at its confirmation line");
        assert_eq!(r.colonize_sacrifices.front().map(|s| s.actor), Some(1), "player 1's entry is now the front, not skipped over");

        // Player 1's own colonize is real (branching: Knights vs Warriors,
        // with a bonus card) -- it must still resolve EXACTLY against the
        // journal's own list, not fall back to approximation now that the
        // queue is correctly aligned.
        r.state.players[1].techs.insert(knights, crate::state::TechSlot { workers: 1, stored: 0 });
        r.state.players[1].hand_military = CardList::new();
        r.state.players[1].hand_military.push(bonus2);
        crate::interact::colonize(&mut r.state, 1, territory1, 4);
        r.drain_colonize().expect("the journal's own list is legal here");

        assert!(r.state.pending.is_empty(), "the force resolved");
        assert_eq!(r.state.players[1].techs.get(knights).map(|s| s.workers), Some(0), "the Knight was sacrificed");
        assert_eq!(
            r.state.players[1].techs.get(warriors).map(|s| s.workers),
            Some(1),
            "the Warriors (the weakest-first fallback's pick) were kept -- this used the journal's real list, not approximation"
        );
        assert!(!r.colonize_approximated, "player 1's colonization was replayed exactly, not approximated");
    }

    /// REGRESSION (`worker FINALINPUT`, chasing game `7522866`'s three-way
    /// final-award divergence on Population/Competition/Variety, all
    /// seat0/Orange -- `analysis/worker_notes_2026-08-14/
    /// finalinput__FINALINPUT.txt`): the fix above
    /// (`a_fully_forced_colonize_pops_its_own_queue_entry_...`) pops a
    /// colonize's queue entry at its own confirmation line whenever
    /// `Pending::Colonize` reads "not open". That is also EXACTLY what "the
    /// auction for this territory has not resolved yet" looks like when this
    /// confirmation line is reached before `resolve_intervening`'s own
    /// forced-`BidPass` has settled the auction (traced on 7522866's own
    /// "Orange colonizes a Developed Territory" line: `interact::colonize`
    /// for that exact territory had not been called yet at the moment this
    /// dispatch ran). Popping there steals the real, still-owed entry;
    /// `drain_colonize`, later, finds the FOLLOWING territory's entry at the
    /// front instead and force-feeds its clauses against the wrong pending,
    /// falling back to `approximate_colonize` -- which can sacrifice a
    /// different unit than the human actually did, permanently undercounting
    /// that player's own worker/army state for every later final-scoring
    /// formula that reads `p.techs` directly.
    ///
    /// The fix gates the pop on a third, purely structural signal that is
    /// false in this shape and true only once `interact::gain_colony` has
    /// actually run: `card` (the confirmation line's own territory) is
    /// already in `p.colonies`.
    #[test]
    fn a_colonize_confirmation_line_reached_before_its_own_auction_has_resolved_does_not_steal_the_queue_entry() {
        let card_index = build_card_index();
        let territory0 = card_index["Vast Territory (I)"];
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 50,
            actor: 0,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(card_index["Warriors"])],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        r.current_lineno = 50;

        // No `interact::colonize` call at all -- unlike the fully-forced
        // case above, `Pending::Colonize` has never existed for this
        // territory: the auction has not resolved yet, exactly the "same-
        // timestamp `WinAuction`/forced-`BidPass`" ordering 7522866 hit.
        assert!(r.state.pending.is_empty(), "the auction has not even resolved yet");
        assert!(!r.state.players[0].colonies.contains(territory0), "and the colony is not won yet either");

        let out = apply_one(&mut r, 0, ActionClass::Colonize, Some(territory0), "colonizes a Vast Territory", "Orange colonizes a Vast Territory Sacrificed Units:; 1 Warrior; Total force: 1", None);
        assert!(out.is_ok(), "{out:?}");
        assert_eq!(
            r.colonize_sacrifices.len(),
            1,
            "the entry must still be here for drain_colonize to consume once the real \
             Pending::Colonize actually opens -- popping it now (the pre-fix behaviour) \
             would misattribute the NEXT territory's clauses to this one once it opens"
        );
    }

    /// REGRESSION: the winner's hidden bonus cards must be grounded while
    /// the auction is still OPEN. `interact::colonize` snapshots the hand
    /// into `Pending::Colonize::bpool` the instant the auction settles, so a
    /// card grounded any later can never be sent -- and the force has to be
    /// made up out of army units the human never spent.
    ///
    /// Also pins the lookup to `Card::base_name`: `Card::name` carries the
    /// age suffix (`"Vast Territory (I)"`) that BGO's journal never prints,
    /// so matching on it makes this method silently never fire.
    #[test]
    fn the_auction_winners_named_bonus_cards_are_grounded_before_the_auction_settles() {
        let card_index = build_card_index();
        let bonus3 = colonization_bonus_card(3).unwrap();
        let territory = card_index["Vast Territory (I)"];
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 107,
            actor: 1,
            territory: "Vast Territory".to_string(),
            clauses: vec![SacrificeClause::Unit(card_index["Warriors"]), SacrificeClause::Bonus(bonus3)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        r.state.players[1].hand_military = CardList::new();
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[1, 0], 1, 3, Some(1), 0)));

        r.ground_auction_winner_hand();

        assert!(r.state.players[1].hand_military.contains(bonus3), "the winner's own named bonus card");
        assert!(!r.state.players[0].hand_military.contains(bonus3), "nobody else's hand is touched");
        assert!(r.colonize_sacrifices.front().is_some(), "grounding only PEEKS -- the drain still owes this entry");
    }

    /// The guard that keeps the peek honest: an auction every player passes
    /// produces no `"colonizes"` line at all, so the queue front belongs to
    /// some LATER auction and its cards must not be handed out now.
    #[test]
    fn a_queued_sacrifice_for_a_different_territory_grounds_nothing() {
        let card_index = build_card_index();
        let bonus3 = colonization_bonus_card(3).unwrap();
        let mut sacrifices = VecDeque::new();
        sacrifices.push_back(ColonizeSacrifice {
            lineno: 107,
            actor: 1,
            territory: "Wealthy Territory".to_string(),
            clauses: vec![SacrificeClause::Bonus(bonus3)],
        });
        let mut r =
            Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), sacrifices);
        r.state.players[1].hand_military = CardList::new();
        let elsewhere = card_index["Vast Territory (I)"];
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(elsewhere, &[1, 0], 1, 3, Some(1), 0)));

        r.ground_auction_winner_hand();

        assert!(r.state.players[1].hand_military.is_empty(), "a different auction's winnings stay hidden");
    }

    /// §11.2 caps a bid at the bidder's own maximum colonization force and
    /// BGO enforces that cap client-side, so a logged bid is PROOF the
    /// bidder could pay it. Player 0 can send exactly their starting
    /// Warrior (force 1) out of everything this binary can see, and the
    /// journal has them raising to 3 -- the missing 2 has to be sitting in
    /// the SIMULATED filler of their military hand, so it is grounded there
    /// and the bid goes through as a real, legal engine move.
    #[test]
    fn a_logged_bid_is_taken_as_proof_the_bidder_held_the_force_to_pay_it() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.age_military = crate::Age::I;
        let territory = card_index["Vast Territory (I)"];
        let filler = card_index["Aggression: Raid (I)"];
        r.state.players[0].hand_military = CardList::new();
        for _ in 0..3 {
            r.state.players[0].hand_military.push(filler);
        }
        assert_eq!(crate::interact::max_force(&r.state, &r.state.players[0]), 1, "one starting Warrior, no bonus cards");
        r.state.pending.push(Pending::Auction(crate::state::Auction::restore(territory, &[0, 1], 0, 2, Some(1), 0)));

        apply_one(&mut r, 0, ActionClass::Bid, None, "bids 3", "Orange bids 3", None).expect("the logged bid is legal");

        let Some(Pending::Auction(a)) = r.state.pending.top() else { panic!("the auction is still open") };
        assert_eq!(a.bid, 3, "the human's own bid was applied through the engine");
        assert_eq!(r.state.players[0].hand_military.len(), 3, "hand SIZE is modelled exactly and must not grow");
        // Age I is the newest bonus card the military deck could have dealt
        // -- two +1s, not one +2, because a +2 card does not exist yet.
        let bonuses = crate::interact::bonus_pool(&r.state.players[0]);
        assert_eq!(bonuses.len(), 2);
        assert!(bonuses.as_slice().iter().all(|id| id.get().age == crate::Age::I), "never a card newer than the deck");
        assert_eq!(r.bid_ceilings_grounded, 2, "reported, not swallowed");
    }

    /// The claim is kept as small as the bid actually pins down: once the
    /// deck has reached age II, a shortfall of 2 is one +2 card, not two
    /// +1s. Fewest cards first, then smallest printed value.
    #[test]
    fn grounding_a_bid_claims_the_fewest_and_smallest_bonus_cards_that_close_the_gap() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.age_military = crate::Age::III; // every bonus card is available
        let filler = card_index["Aggression: Raid (I)"];
        r.state.players[0].hand_military = CardList::new();
        for _ in 0..3 {
            r.state.players[0].hand_military.push(filler);
        }

        assert!(r.ground_bid_ceiling(0, 3), "one Warrior plus one +2 card reaches 3");

        let bonuses = crate::interact::bonus_pool(&r.state.players[0]);
        assert_eq!(bonuses.len(), 1, "one card claimed, not two");
        assert_eq!(bonuses.as_slice()[0].get().effects.colonization_bonus, 2, "the smallest that closes the gap");
    }

    /// A hand card the journal later shows this player PLAYING is one of
    /// the few military-hand slots that is NOT filler -- overwriting it
    /// would contradict a fact the journal already states. Same predicate
    /// (`DiscardSolver::needed_after`) a forced hand-limit discard uses,
    /// called rather than re-implemented.
    #[test]
    fn grounding_a_bid_never_overwrites_a_hand_card_the_journal_shows_played_later() {
        let card_index = build_card_index();
        let raid = card_index["Aggression: Raid (I)"];
        let mut needs: HashMap<u8, Vec<FutureNeed>> = HashMap::new();
        needs.insert(0, vec![FutureNeed { lineno: 900, card: raid }]);
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), needs, VecDeque::new());
        r.state.age_military = crate::Age::III;
        r.current_lineno = 100; // the play is still ahead of us
        r.state.players[0].hand_military = CardList::new();
        r.state.players[0].hand_military.push(raid);

        assert!(!r.ground_bid_ceiling(0, 3), "the only hand card is spoken for");
        assert!(r.state.players[0].hand_military.contains(raid), "and is left exactly where it was");
    }

    /// `docs/REPLAY.md`'s Take/HandFull "genuinely unexplained discrepancy"
    /// conclusion: a journal-observed take rejected ONLY by `hand_full`
    /// (cost affordable, no wonder in progress, no duplicate name, no
    /// leader-age conflict) is a fact about what BGO's real implementation
    /// permitted, so `take_blocked_only_by_hand_full` must say so.
    #[test]
    fn take_blocked_only_by_hand_full_is_true_when_hand_full_is_the_sole_rejecting_gate() {
        let card_index = build_card_index();
        let r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        let mut r = r;
        r.state.players[0].government = card_index["Despotism"];
        r.state.players[0].civil_actions = 10;
        r.state.players[0].hand_civil = CardList::new();
        for _ in 0..4 {
            r.state.players[0].hand_civil.push(card_index["Bronze"]); // fills the 4-CA Despotism limit
        }
        r.state.card_row[0] = card_index["Selective Breeding"];
        assert!(
            take_blocked_only_by_hand_full(&r.state, &r.state.players[0], 0),
            "affordable, no duplicate, no wonder, no leader -- hand_full is the only gate in play"
        );
    }

    /// The narrowness requirement: if a SECOND gate would also reject the
    /// same take (here, `DuplicateCard` -- the row card is already in
    /// hand), `take_blocked_only_by_hand_full` must say `false` and leave
    /// the ordinary, honest `IllegalMove: Take` mismatch in place. Proven
    /// by probing `costs::take_rejection` a second time with `hand_full`
    /// forced off, exactly like the function under test does, never by a
    /// parallel reimplementation of the gate order.
    #[test]
    fn take_blocked_only_by_hand_full_is_false_when_a_second_gate_also_rejects_the_same_take() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].government = card_index["Despotism"];
        r.state.players[0].civil_actions = 10;
        r.state.players[0].hand_civil = CardList::new();
        r.state.players[0].hand_civil.push(card_index["Selective Breeding"]);
        for _ in 0..3 {
            r.state.players[0].hand_civil.push(card_index["Bronze"]); // fills to 4/4
        }
        r.state.card_row[0] = card_index["Selective Breeding"]; // already in hand: DuplicateCard too
        assert!(
            !take_blocked_only_by_hand_full(&r.state, &r.state.players[0], 0),
            "hand_full is not the ONLY blocking gate here -- must not override"
        );
    }

    /// A take whose COST alone already exceeds the budget must not be
    /// treated as a hand_full-only case even if the hand also happens to
    /// be full -- `Budget` is checked (and short-circuits) before
    /// `hand_full` in `costs::take_rejection`'s own branch order, so this
    /// pins that `take_blocked_only_by_hand_full`'s first check (the real
    /// gate must name `HandFull` specifically) actually does its job.
    #[test]
    fn take_blocked_only_by_hand_full_is_false_when_the_cost_alone_is_unaffordable() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.players[0].government = card_index["Despotism"];
        r.state.players[0].civil_actions = 0; // cannot afford any row slot
        r.state.players[0].hand_civil = CardList::new();
        for _ in 0..4 {
            r.state.players[0].hand_civil.push(card_index["Bronze"]);
        }
        r.state.card_row[0] = card_index["Selective Breeding"];
        assert!(!take_blocked_only_by_hand_full(&r.state, &r.state.players[0], 0));
    }

    /// End-to-end through the real call site: `Replayer::try_apply_take`
    /// accepts a hand_full-only take that `legal::legal_moves` (the real
    /// engine, untouched) refuses, applies it via `apply::apply` exactly
    /// like an ordinary legal `Move::Take` would be, and counts it in
    /// `hand_full_takes_overridden` -- "counted and visible", never a
    /// silent pass.
    #[test]
    fn try_apply_take_accepts_and_counts_a_hand_full_only_take_the_engine_refuses() {
        let card_index = build_card_index();
        let mut r = Replayer::new(&card_index, 2, EventPlan::default(), HashMap::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new(), HashMap::new(), HashMap::new(), HashMap::new(), VecDeque::new());
        r.state.phase = Phase::Actions;
        r.state.current = 0;
        r.state.players[0].government = card_index["Despotism"];
        r.state.players[0].civil_actions = 10;
        r.state.players[0].hand_civil = CardList::new();
        for _ in 0..4 {
            r.state.players[0].hand_civil.push(card_index["Bronze"]);
        }
        r.state.card_row[0] = card_index["Selective Breeding"];
        assert!(
            !legal::legal_moves(&r.state).as_slice().contains(&Move::Take { slot: 0 }),
            "the engine, untouched, must still call this illegal"
        );

        r.try_apply_take(0, 0).expect("the replayer-only divergence accepts it");

        assert_eq!(r.hand_full_takes_overridden, 1, "reported, not swallowed");
        assert!(
            r.state.players[0].hand_civil.contains(card_index["Selective Breeding"]),
            "the card actually landed in hand"
        );
    }
}
