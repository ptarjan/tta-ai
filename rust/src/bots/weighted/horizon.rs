//! `engine/bots/weighted.py` lines 1056-1415: the game horizon -- how many
//! rounds are left, how much of the civil supply is unseen, how "late" the
//! game is, and what a per-turn RATE feature is worth given how much game is
//! left to collect it in. See that Python range's own extensive comments
//! (`_tail`/`_supply`/`take_rate`/`rounds_left`/`lateness`/`rate_multiplier`
//! each carry a paragraph or more on WHY the arithmetic is shaped the way it
//! is -- rule-derived vs. measured vs. fitted, and what each replaced) for
//! the derivations; restated here only where the Rust shape earns its own
//! note.
//!
//! ## No memo caches -- but also no repeated scans, since the answer never
//! ## changes
//!
//! Python's `_TAIL`/`_SUPPLY` are module-level dicts, lazily filled and kept
//! forever, because `C.db().civil_deck(age, n)` -- a dict-of-dicts walk -- is
//! "far too slow for the search loop" (that module's own comment). This port
//! still keeps the house rule those caches broke (no process-global mutable
//! state, no lazily-populated global cache) but does NOT pay
//! [`crate::game::build_deck`]'s cost to answer [`tail`]/[`supply`], because
//! that cost turned out not to be optional to avoid: profiling `PlanBot`
//! single-threaded (`sample` on a release `kindmatch` run, 2026-08-06) found
//! [`supply`] as the single hottest leaf in the whole search by a wide
//! margin, with [`cards_unseen`] (which calls [`tail`]) close behind.
//! [`horizon::lateness`](lateness) alone is called from six independent
//! sites across `bots/weighted/{eval,row,rivals,cards}.rs` -- several of them
//! per CARD being priced, not per position -- so a single [`super::eval::
//! evaluate`] call can re-run [`supply`]/[`tail`] a double-digit number of
//! times, and a beam search calls `evaluate` at every node. `build_deck`
//! itself is not the "single filtering pass, no heap allocation" this
//! comment used to claim either: for every matching card it PUSHES one
//! [`crate::cards::CardId`] per copy into a [`crate::state::CardList`] the
//! caller here only ever calls `.len()` on and immediately discards --
//! real writes for a length nobody wanted.
//!
//! The fix is not a cache (nothing is computed once and remembered across
//! calls at runtime); it is that [`tail`]/[`supply`] never needed to call
//! `build_deck` at all. Civil deck composition depends on nothing but
//! `(age, player count)` -- both drawn from the same printed, checked-in card
//! data [`crate::card_table::CARDS`] already bakes at compile time -- so
//! [`CIVIL_DECK_LEN`] bakes the 4x3 answer as a literal table, the same
//! "parse nothing, allocate nothing at start-up" choice `Cargo.toml`'s own
//! comment on the empty `[dependencies]` describes for `card_table.rs`
//! itself. [`tests::civil_deck_len_matches_build_deck_for_every_age_and_
//! player_count`] pins the table against `build_deck`'s own output so a
//! future edit to `data/*.json`/`card_table.rs` that changes a civil card's
//! `count` fails loudly here instead of silently going stale.
//!
//! [`crate::bots::counting`]'s own `civil_outlook` still calls `build_deck`
//! directly -- it needs actual card IDENTITIES (which ages/cards remain
//! unseen), not just a length, so there is no length-only shortcut for it to
//! take.
//!
//! ## `_ROW` and `horizon_scale`'s `w` parameter: two dead-code fixes
//!
//! While surveying this range for the port, two artifacts turned out to be
//! genuinely unread and were fixed in the Python original alongside this
//! port (repo owner's ruling: fix a found defect in both engines rather than
//! carry it forward for shape fidelity):
//!
//! * `_ROW = actions.ROW_SIZE` used to sit next to `_SWEEP`/`AGE_IV_ROUNDS`
//!   here. Grepping the whole tree for the name turns up only its own
//!   definition -- nothing ever read it. Not ported; see
//!   `tests/test_rate_horizon.py::DeadCodeFoundWhilePortingToRust` in the
//!   Python tree.
//! * `horizon_scale(state, n=None, w=None)` accepted a `w` weight dict and
//!   never referenced it in its body. [`horizon_scale`] below takes only
//!   `state`/`n`.
//!
//! Neither changes any number this module or Python's `evaluate` produces --
//! both were parameters/constants nothing downstream ever read.

use crate::cards::Age;
use crate::game;
use crate::state::{GameState, PlayerState};

use super::weights::{WeightKey, Weights};

/// RULE-DERIVED, RULES_SPEC 12.3: once Age IV begins, the game ends this
/// round or the next.
pub const AGE_IV_ROUNDS: f64 = 2.0;

/// FITTED PRIOR (see docs/EVALUATOR_HISTORY.md), the only fitted number left in
/// the horizon: cards taken off the row per replenish before this game has
/// produced any evidence of its own, by live player count. Measured over 240
/// self-play games (`tools/deal_rate.py`, deleted 2026-08-04, Python side).
/// Indexed by `n - 2` (2p/3p/4p), mirroring `Card::count`'s own `[u8; 3]`
/// convention in `cards.rs`.
const TAKE_PRIOR: [f64; 3] = [0.30, 0.35, 0.40];

/// `TAKE_PRIOR`'s weight in pseudo-replenishes -- shrunk away within a
/// couple of rounds, so it moves the estimate only in Age A and the first
/// rounds of Age I.
const TAKE_PRIOR_W: f64 = 4.0;

/// The four per-turn RATE features [`rate_multiplier`] scales. Deliberately
/// excludes `rival_culture_rate`/`rival_science_rate` -- those are
/// max-over-rivals THREAT signals the coordinate registry declares inert
/// across a candidate set, and scaling them would vary between candidates as
/// a side effect nobody asked for. See Python's own comment on `RATE_KEYS`
/// for the full argument.
pub const RATE_KEYS: &[WeightKey] =
    &[WeightKey::CultureRate, WeightKey::ScienceRate, WeightKey::FoodRate, WeightKey::ResourceRate];

/// `_live`: live player count, clamped to the 2-4 range every composition
/// lookup below assumes (RULES_SPEC 13).
///
/// Deliberately NOT [`crate::bots::counting::live_count`] (private to that
/// module in any case): that one is UNCLAMPED on purpose, because Python's
/// `Db._deck` simply returns nothing for a count key that is not `"2p"`/
/// `"3p"`/`"4p"` -- an empty composition, not a crash. [`game::build_deck`]
/// instead computes `num_players - 2` to index `Card::count`, which
/// underflows below 2 players, so every caller here needs the clamp Python's
/// `_live` applies (`2 if n < 2 else (4 if n > 4 else n)`).
///
/// `pub`, not `_live`-private: [`crate::bots::counting`]'s own top doc
/// comment requires callers to compute a value like this once at the search
/// root and thread it down rather than recomputing it deep in a search --
/// which requires every submodule under `bots::weighted` to be able to reach
/// it, not just this file.
pub fn live_count(state: &GameState) -> usize {
    let n = state.players[..state.num_players as usize].iter().filter(|p| !p.resigned).count();
    n.clamp(2, 4)
}

/// Civil deck size, by age index (`Age::A = 0` through `Age::III = 3`) and
/// `n - 2` (2p/3p/4p) -- the exact counts `game::build_deck(age, true, n).len()`
/// returns, baked once rather than recomputed by scanning+pushing every
/// matching card out of `card_table::CARDS` on every call. See this module's
/// top doc comment for why this is baked data, not a runtime cache, and
/// [`tests::civil_deck_len_matches_build_deck_for_every_age_and_player_
/// count`] for the proof this cannot silently drift from `card_table.rs`.
const CIVIL_DECK_LEN: [[u32; 3]; 4] = [
    [20, 20, 20], // Age A
    [44, 50, 53], // Age I
    [44, 50, 53], // Age II
    [44, 50, 53], // Age III
];

/// `_tail`: civil cards left in every age's deck strictly AFTER `age`, for
/// `n` players -- EXACT, from the same printed card data `game::build_deck`
/// deals from, but read out of the baked [`CIVIL_DECK_LEN`] table rather than
/// rescanning `card_table::CARDS`. See this module's top doc comment.
///
/// `age as usize` is [`CIVIL_DECK_LEN`]'s own row index (`Age::A = 0` through
/// `Age::III = 3`) -- `age` is NOT guaranteed to be one of those four:
/// `age` is `state.age_civil`, which reaches `Age::IV` in every Age IV
/// position this is called from (`cards_unseen`/`lateness`/`rate_multiplier`
/// do not stop being called once the deck runs out). `from` is then `5`,
/// past [`CIVIL_DECK_LEN`]'s four rows, and slicing an empty-or-past-the-end
/// range with `.get` rather than indexing directly returns `None` there --
/// correctly zero cards left after the last age, matching the pre-baked-table
/// code's own `CIVIL_AGES.iter().filter(|&a| a as u8 > age as u8)` (which
/// also silently yielded nothing once `age` was at or past `III`).
fn tail(n: usize, age: Age) -> u32 {
    debug_assert!((2..=4).contains(&n), "n must be a live player count 2..=4, got {n}");
    let from = age as usize + 1;
    CIVIL_DECK_LEN.get(from..).unwrap_or(&[]).iter().map(|row| row[n - 2]).sum()
}

/// `_supply`: `(total civil cards for `n` players across ages A-III, size of
/// the Age A deck alone)`. Both exact card data, read out of [`CIVIL_DECK_LEN`].
fn supply(n: usize) -> (u32, u32) {
    debug_assert!((2..=4).contains(&n), "n must be a live player count 2..=4, got {n}");
    let total = CIVIL_DECK_LEN.iter().map(|row| row[n - 2]).sum();
    let age_a = CIVIL_DECK_LEN[0][n - 2];
    (total, age_a)
}

/// `cards_unseen`: civil cards still to be dealt -- EXACT, the current deck
/// plus every later age's deck.
pub fn cards_unseen(state: &GameState, n: usize) -> u32 {
    state.civil_deck.len() as u32 + tail(n, state.age_civil)
}

/// `_replenishes`: how many times the top-of-turn replenish has run --
/// EXACT. `state.turn` is a 1-based player-turn counter and replenishing
/// starts from round 2, so this is the turn counter less the first round's
/// `num_players` turns. Can be negative during round 1, which every caller
/// treats as "no history yet" (see [`take_rate`]).
fn replenishes(state: &GameState) -> i32 {
    state.turn as i32 - state.num_players as i32
}

/// `take_rate`: cards players take off the row per replenish, MEASURED in
/// this game (not a fitted constant, past the first round or two -- see
/// Python's `take_rate` for the full derivation of `consumed`/`taken`).
pub fn take_rate(state: &GameState, n: usize) -> f64 {
    debug_assert!((2..=4).contains(&n), "n must be a live player count 2..=4, got {n}");
    let r = replenishes(state);
    let prior = TAKE_PRIOR[n - 2];
    if r <= 0 || state.age_civil == Age::A {
        return prior;
    }
    let (total, age_a) = supply(n);
    let consumed = total as f64 - cards_unseen(state, n) as f64;
    let taken = (consumed - age_a as f64 - f64::from(r) * game::sweep_count(n) as f64).max(0.0);
    (taken + TAKE_PRIOR_W * prior) / (f64::from(r) + TAKE_PRIOR_W)
}

/// `rounds_left`: estimated rounds still to play, including the one in
/// progress. Exact once Age IV has begun (`state.final_round_end` is set);
/// before that, the EXACT undealt-card count divided by a deal rate MEASURED
/// in this game. Never below 1.0.
pub fn rounds_left(state: &GameState, n: usize) -> f64 {
    if let Some(fre) = state.final_round_end {
        return (f64::from(fre) - f64::from(state.round) + 1.0).max(1.0);
    }
    debug_assert!((2..=4).contains(&n), "n must be a live player count 2..=4, got {n}");
    let cards = cards_unseen(state, n) as f64;
    let per_round = n as f64 * (game::sweep_count(n) as f64 + take_rate(state, n));
    (cards / per_round + AGE_IV_ROUNDS).max(1.0)
}

/// The age at whose END a card of age `age` is culled from play, or `None`
/// when the game itself ends first.
///
/// RULES_SPEC 12.2 / [`game::antiquate`]: at the end of age `ended`,
/// everything with `age < ended` leaves play. A card of age `a` therefore
/// SURVIVES the end of its own age (`a >= a`) and dies at the end of age
/// `a + 1`. An Age III card's deadline would be the end of Age IV, which is
/// the end of the game (RULES_SPEC 12.3) -- there is no antiquation deadline
/// short of the game's own, which is what `None` means here.
///
/// A `match` over every [`Age`] with no wildcard arm, rather than `a + 1`
/// arithmetic on the discriminant: a fifth age would then be a compile error
/// here instead of an off-by-one that silently prices the wrong deadline.
const fn antiquated_at_end_of(age: Age) -> Option<Age> {
    match age {
        Age::A => Some(Age::I),
        Age::I => Some(Age::II),
        Age::II => Some(Age::III),
        Age::III | Age::IV => None,
    }
}

/// The position-level facts every "when is a card of age `a` culled" question
/// needs, read off the board ONCE.
///
/// THE HOLE THIS FILLS: [`rounds_left`] is the deadline for the GAME, and it
/// was the only deadline the evaluator had. For an Age A wonder taken in Age A
/// it overstates the real one by two whole ages -- the wonder is gone at the
/// end of Age I whether or not the game is still running, resources sunk and
/// lost (RULES_SPEC 12.2, [`crate::game`]'s `antiquate`). That is a rule, not
/// a preference, and it appeared nowhere in the weight vector.
///
/// A struct rather than a free function called once per card because the
/// caller that matters is a whole HAND: `features()` asks this question for
/// every civil card it holds, on the evaluator's hot path, and this module's
/// own top doc comment records [`supply`]/[`tail`] as the single hottest leaf
/// in the search. Everything expensive ([`rounds_left`], [`take_rate`], and
/// the deal rate they share) depends on the POSITION and not on the card, so
/// it is computed once here and the per-card answer is arithmetic on the baked
/// [`CIVIL_DECK_LEN`] table. Not a cache: nothing is remembered across calls,
/// it is a value the caller holds for as long as it is asking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AntiquationClock {
    /// What is left of the age currently being dealt.
    civil_deck: u32,
    /// The age currently being dealt (`state.age_civil`).
    age_civil: Age,
    /// Cards dealt per round: `n * (sweep_count + take_rate)` -- the identical
    /// denominator [`rounds_left`] divides by, not a second notion of it.
    per_round: f64,
    /// [`rounds_left`] at this position, the cap every answer is clamped to.
    rounds_left: f64,
    /// Live player count, this struct's [`CIVIL_DECK_LEN`] column.
    n: usize,
}

impl AntiquationClock {
    pub fn at(state: &GameState, n: usize) -> AntiquationClock {
        debug_assert!((2..=4).contains(&n), "n must be a live player count 2..=4, got {n}");
        AntiquationClock {
            civil_deck: state.civil_deck.len() as u32,
            age_civil: state.age_civil,
            per_round: n as f64 * (game::sweep_count(n) as f64 + take_rate(state, n)),
            rounds_left: rounds_left(state, n),
            n,
        }
    }

    /// [`rounds_left`] at the position this clock was built from -- carried so
    /// a caller holding the clock never has to recompute (or re-derive) it.
    pub fn rounds_left(&self) -> f64 {
        self.rounds_left
    }

    /// Rounds until a card of age `age` is culled from play at an age
    /// boundary ([`antiquated_at_end_of`]).
    ///
    /// Same arithmetic as [`rounds_left`], deliberately: the cards that still
    /// have to be dealt before the boundary, over the identical deal rate,
    /// read out of the identical baked [`CIVIL_DECK_LEN`] table. Nothing here
    /// is fitted.
    ///
    /// Clamped to `[0, rounds_left]`: a deadline past the end of the game is
    /// the end of the game, and a deadline already behind us (an age older
    /// than the one being dealt -- reachable only defensively, since such a
    /// card has already left play) is zero.
    pub fn rounds_until_antiquation(&self, age: Age) -> f64 {
        let Some(deadline) = antiquated_at_end_of(age) else {
            // The game ends before any boundary could take this card.
            return self.rounds_left;
        };
        if (deadline as u8) < (self.age_civil as u8) {
            return 0.0;
        }
        // Cards still to be dealt before that boundary: what is left of the
        // age being dealt now, plus every whole age's deck between it and the
        // deadline age.
        let from = self.age_civil as usize + 1;
        let to = deadline as usize;
        let future: u32 = match CIVIL_DECK_LEN.get(from..=to) {
            Some(rows) => rows.iter().map(|row| row[self.n - 2]).sum(),
            None => 0,
        };
        (f64::from(self.civil_deck + future) / self.per_round).clamp(0.0, self.rounds_left)
    }
}

/// `rounds_to_antiquation`: one card's deadline at one position. The whole
/// formula lives in [`AntiquationClock::rounds_until_antiquation`]; a caller
/// asking about more than one card should build the clock once instead of
/// calling this in a loop (see that struct's doc comment).
pub fn rounds_to_antiquation(state: &GameState, age: Age, n: usize) -> f64 {
    AntiquationClock::at(state, n).rounds_until_antiquation(age)
}

/// `lateness`: how far through the game we are, 0.0 at the deal, 1.0 when
/// the civil supply is gone. EXACT -- no rate, no fit, no player-count table.
///
/// Clamped to `[0, 1]`, and the clamp is load-bearing: Python's own doc
/// comment on this function measures what an unclamped `1 - L` does near the
/// very end of the game (it goes negative and flips the sign of every
/// early-phase weight). `f64::clamp` is that same guard, not an
/// approximation of it.
pub fn lateness(state: &GameState) -> f64 {
    let n = live_count(state);
    let (total, _) = supply(n);
    let lv = 1.0 - cards_unseen(state, n) as f64 / f64::from(total);
    lv.clamp(0.0, 1.0)
}

/// RULE-DERIVED, RULES_SPEC 5.0/6.6.4: the earliest round in which ANY
/// player's OWN Politics Phase can ever feature a military card that
/// player actually holds. Two cited facts, chained, not fitted:
///
/// 1. No military card is drawn before round 2 (6.6.4, "none on round 1"),
///    so `FIRST_DRAW_ROUND = 2` is the earliest ANY hand can gain a card at
///    all (drawing happens in the END-of-turn sequence, so it is that
///    player's round-2 turn that first produces one).
/// 2. A card gained at the end of a turn cannot be played that same turn --
///    the Politics Phase (5.0) where a political action is taken already
///    happened earlier in that same turn's sequence -- so the earliest it
///    can be PLAYED is that SAME player's next own turn, exactly one round
///    later (5.0: one turn per player per round, fixed order).
///
/// `EARLIEST_COMBAT_ROUND = FIRST_DRAW_ROUND + 1 = 3`.
const FIRST_DRAW_ROUND: u16 = 2;
pub const EARLIEST_COMBAT_ROUND: u16 = FIRST_DRAW_ROUND + 1;

/// RULE-DERIVED, RULES_SPEC 1.4/1.6/5.1-5.6/6.6.4/`EARLIEST_COMBAT_ROUND`:
/// true while no active player can possibly hold an aggression or war card
/// THAT THEY COULD ALREADY ACT ON, i.e. combat is not merely unlikely but
/// structurally impossible this turn.
///
/// Two parts, both load-bearing:
///
/// * `state.round < EARLIEST_COMBAT_ROUND` -- a proven LOWER BOUND (see that
///   constant's own derivation), independent of any specific game's actual
///   draws. This half exists because raw current hand size ALONE is too
///   eager within a round: turn order is fixed, so by the time the LAST
///   seat to act in a round takes their turn, every EARLIER seat has
///   already finished theirs, including its end-of-turn military draw --
///   e.g. in a real round-2 2p game, seat 0 (2 unused MAs under Despotism)
///   ends its round-2 turn holding 2 military cards, cards that will not be
///   PLAYABLE by seat 0 until round 3, before seat 1 has even made its own
///   round-2 build decision. A bare hand-size check would call combat
///   "reachable" for seat 1 right there, reopening the overvaluation this
///   whole gate exists to close, for a card that cannot be played by anyone
///   for another full round. Confirmed by direct measurement, not assumed:
///   `combat_unreachable` without this floor measured 47.2% military-first
///   at 2p (STRGATE.txt section 7), against 0.0% with it.
/// * `state.active().all(|p| p.hand_size_military() == 0)` -- once past that
///   proven floor, falls back to the actual public fact instead of
///   continuing to guess: an aggression or war card can only ever reach a
///   hand via a military-deck draw (5.4/5.6, both "reveal" a card already
///   in hand), and no Age A military card is EVER dealt to a hand at all
///   (1.4/1.6 -- Age A's military deck is shuffled once, at setup, straight
///   into the current-events deck, and its remainder goes "to the box
///   unseen"). So this half can only ever EXTEND the unreachable window
///   past the floor (never shrink it below the floor), for the actual
///   games where nobody has drawn anything yet.
///
/// PUBLIC INFORMATION ONLY: the second half reads
/// [`PlayerState::hand_size_military`] (a COUNT), never a hand's contents.
/// A rival's military hand is kept face down but its SIZE is not secret --
/// `state.rs`'s own `hidden_military` field exists specifically because the
/// app harness mirrors a real opponent whose hand size is known even when
/// its contents were not transcribed (see that field's doc comment) -- so
/// this is the same public/private boundary the engine already draws
/// elsewhere, not a new one invented for this function. `state.round` is
/// global public state. Legal for every call site regardless of which
/// player `idx` is being evaluated, because it never looks at any player's
/// hand contents, including the evaluated player's own.
///
/// This is intentionally NOT "no rival has built military strength yet":
/// that would be REACTIVE (it degenerates in self-play -- if nobody ever
/// builds military, no rival ever shows strength, so the gate never closes,
/// so strength never gets valued, so nobody ever builds it) and would read
/// built units, which are a strategic CHOICE, not a rules-fixed fact.
/// Military-card draws are not a choice: they happen automatically off
/// unused military actions at every end of turn regardless of a player's
/// strategy (6.6.4), so this gate closes on a fixed schedule even in a
/// fully pacifist self-play population, the same way lateness/rounds_left
/// do -- and `EARLIEST_COMBAT_ROUND` itself is a fixed structural constant,
/// not a per-game observation, so it cannot be gamed by any strategy either.
pub fn combat_unreachable(state: &GameState) -> bool {
    state.round < EARLIEST_COMBAT_ROUND || state.active().all(|p| p.hand_size_military() == 0)
}

/// `horizon_scale`: `rounds_left`, normalised so an average-moment decision
/// scores 1.0. Mean ~1.0 over a game by construction; ~1.9 at the deal,
/// ~0.09 on the last turn. Never negative, because `rounds_left` never is.
pub fn horizon_scale(state: &GameState, n: usize) -> f64 {
    let rl = rounds_left(state, n);
    let reference = 0.5 * (rl + (f64::from(state.round) - 1.0).max(0.0) + 1.0);
    if reference > 0.0 {
        rl / reference
    } else {
        1.0
    }
}

/// `rate_multiplier`: what [`RATE_KEYS`] features are multiplied by. `1.0`
/// (no-op) when [`WeightKey::RateHorizon`] is exactly `0.0`; otherwise a
/// blend between flat pricing (0.0) and the full `rounds_left / mean`
/// horizon (1.0), floored at 0.0 so a credit above 1.0 can flatten a rate but
/// never invert its sign.
pub fn rate_multiplier(state: &GameState, weights: &Weights, n: usize) -> f64 {
    let c = weights.get(WeightKey::RateHorizon);
    if c == 0.0 {
        return 1.0;
    }
    (1.0 + c * (horizon_scale(state, n) - 1.0)).max(0.0)
}

// ------------------------------------------------ the wonder under construction
//
// The horizon question a wonder asks is not "how many rounds are left" but
// "how many rounds am I going to OWN this thing for" -- and both halves of
// that (when it finishes, when the game ends) are computable from the board,
// so neither is a weight's job. This section owns that one computation for
// every caller; see [`WonderOutlook`]'s own doc comment.

/// NUMERICAL GUARD, not a model claim (mirrors Python's `_TURNS_CAP`):
/// [`WonderOutlook::turns_to_finish`] is a ratio that blows up as resource
/// production approaches zero. 20 turns is already past "never" for a
/// 20-round game, so nothing inside the cap is shaped by it -- it only keeps
/// an infinity from reaching the linear evaluator.
///
/// Lived in `features.rs` until the wonder arithmetic moved here; it is part
/// of the same computation, so it moved with it rather than being restated.
const TURNS_CAP: f64 = 20.0;

/// Everything the evaluator can KNOW about the wonder a player is part-way
/// through, read off the board rather than fitted.
///
/// Every field is a computed board fact. The one that did not exist before
/// is [`WonderOutlook::collect_fraction`]: the share of the rest of the game
/// this wonder would actually be standing, and therefore producing, for. A
/// wonder is not worth what it prints -- it is worth what it prints for as
/// long as you have it, and paying a stage BUYS that time. Nothing in the
/// evaluator used to represent that: `wonder_progress`/`wonder_remaining`
/// are identity-blind stocks (they know how many resources a wonder still
/// owes, not what completing it would DO), and `cards::wonder_potential` is
/// identity-aware but progress-blind (it returns the same number whether you
/// are one stage in or one stage from the end). See `docs/AGREEMENT.md`'s
/// "delayed payoff" section.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WonderOutlook {
    /// Resources already sunk into the stages built so far.
    pub progress: i32,
    /// Resources the unbuilt stages still owe.
    pub remaining: i32,
    /// Stages not yet built. `0.0` once nothing is outstanding.
    pub stages_left: f64,
    /// Turns of this player's WHOLE resource output the wonder still owes,
    /// net of what is already banked -- scale-free, so it means the same
    /// thing to an Age A economy and an Age III one. Capped at
    /// [`TURNS_CAP`]; `0.0` when the outstanding cost is already in the bank.
    pub turns_to_finish: f64,
    /// The part of [`WonderOutlook::turns_to_finish`] the game will not last
    /// long enough to pay -- the "0-for-58" detector.
    pub overrun: f64,
    /// [`rounds_left`] at this position, carried so a caller never has to
    /// recompute (or, worse, re-derive) a second notion of it.
    pub rounds_left: f64,
    /// Rounds the finished wonder would actually be on the board for:
    /// `rounds_left - turns_to_finish`, floored at zero. Rises every time a
    /// stage is paid (paying a stage of cost `c` moves the outstanding cost
    /// down by `c` AND the bank down by `c`, so the shortfall closes by
    /// `2c`), and rises when resource production rises.
    pub collect_rounds: f64,
    /// [`rounds_to_antiquation`] for THIS wonder's age -- the rule-derived
    /// deadline the stages actually have to be paid by (RULES_SPEC 12.2), as
    /// opposed to [`WonderOutlook::rounds_left`], the deadline the game has.
    /// Never above `rounds_left`.
    pub rounds_to_antiquation: f64,
    /// The part of [`WonderOutlook::turns_to_finish`] that falls past the
    /// ANTIQUATION deadline rather than past the end of the game:
    /// `max(0, turns_to_finish - rounds_to_antiquation)`. The sibling of
    /// [`WonderOutlook::overrun`] with the horizon the rules actually impose
    /// -- see [`rounds_to_antiquation`] for why the two differ by up to two
    /// whole ages.
    pub age_overrun: f64,
}

impl WonderOutlook {
    /// [`WonderOutlook::collect_rounds`] as a share of the game that is left
    /// -- `1.0` for a wonder that can be finished out of the bank right now,
    /// `0.0` for one the game will end before you finish. Dimensionless on
    /// purpose: it multiplies a value that is already in evaluator points, so
    /// it changes WHEN that value is collected without re-pricing WHAT it is
    /// worth. [`rounds_left`] is never below `1.0`, so this never divides by
    /// zero.
    pub fn collect_fraction(&self) -> f64 {
        self.collect_rounds / self.rounds_left
    }

    /// The share of this wonder's total printed cost that has actually been
    /// paid: `progress / (progress + remaining)`. `0.0` for a wonder just
    /// taken from the row, `1.0` for one whose last stage is bought.
    ///
    /// This is the OTHER half of the delayed payoff, and the one the
    /// evaluator was missing outright: an unfinished wonder scores nothing
    /// under the rules (RULES_SPEC 12: only completed wonders count), so its
    /// value is not owned on the turn the card is taken -- it is bought,
    /// stage by stage, and each stage buys its own pro-rata share of it.
    /// Booking the whole value up front (which is what a constant `1.0` here
    /// amounts to) makes TAKING a wonder look maximally good and then makes
    /// every stage that pays for it look worth nothing at all, since the term
    /// contributes the identical number to every candidate move from then on.
    ///
    /// Every wonder in the base game prints at least one stage with a nonzero
    /// cost, so the denominator is positive whenever [`wonder_outlook`]
    /// returned `Some`; the guard is there so a data edit that printed a
    /// free wonder could not produce a NaN inside the evaluator.
    pub fn paid_fraction(&self) -> f64 {
        let total = self.progress + self.remaining;
        if total > 0 { f64::from(self.progress) / f64::from(total) } else { 1.0 }
    }

    /// What share of a FINISHED wonder's value an in-progress one has
    /// actually earned on this board: how much of it is paid for
    /// ([`WonderOutlook::paid_fraction`]) times how much of the rest of the
    /// game you would own it for ([`WonderOutlook::collect_fraction`]).
    ///
    /// Both factors are computed board facts, and both move on exactly the
    /// move that ought to move them: paying a stage of cost `c` raises the
    /// first by `c / total` and the second by closing the resource shortfall
    /// by `2c` (the outstanding cost falls by `c` and so does the bank it is
    /// netted against). Nothing here is fitted -- the weight that says how
    /// much a finished wonder is worth relative to everything else is
    /// [`super::weights::WeightKey::WonderPotential`], applied by `eval.rs`
    /// to the value this scales, and it is unchanged.
    pub fn earned_share(&self) -> f64 {
        self.paid_fraction() * self.collect_fraction()
    }

    /// How much of the deadline the wonder would still be standing for if the
    /// player keeps paying: `(rounds_to_antiquation - turns_to_finish) /
    /// rounds_to_antiquation`, clamped to `[0, 1]`.
    ///
    /// [`WonderOutlook::collect_fraction`]'s sibling against the RULES'
    /// deadline instead of the game's. `0.0` for a wonder that cannot be
    /// finished before it is antiquated -- which is the single fact that most
    /// distinguishes a wonder worth taking from one that is 6 sunk resources
    /// (66.5% of every wonder started in the 200-game 2p census died this way).
    /// Zero rather than a divide-by-zero when the deadline has already passed.
    pub fn feasible_fraction(&self) -> f64 {
        if self.rounds_to_antiquation <= 0.0 {
            return 0.0;
        }
        ((self.rounds_to_antiquation - self.turns_to_finish) / self.rounds_to_antiquation)
            .clamp(0.0, 1.0)
    }

    /// The share of a finished wonder's value that is still AHEAD of the
    /// player -- unpaid, but reachable before the antiquation deadline:
    /// `(1 - paid_fraction) * feasible_fraction`.
    ///
    /// The exact complement of [`WonderOutlook::earned_share`], and the reason
    /// both exist: `earned_share` is `paid_fraction * collect_fraction`, so it
    /// is identically `0.0` on the one move that TAKES a wonder from the row
    /// (nothing is paid yet). Every identity-aware thing the evaluator knows
    /// about a wonder is multiplied by that zero, which is why no assignment of
    /// the weight vector could tell Pyramids from Hanging Gardens at take time
    /// (`super::cards::tests::two_wonders_with_different_powers_score_
    /// identically_at_the_moment_they_are_taken`). This factor is at its
    /// MAXIMUM there and decays to zero as stages are paid, handing the value
    /// across to `earned_share` over the same interval.
    ///
    /// Both factors are computed board facts and both are shares, so the
    /// product is in `[0, 1]`: the evaluator can promise at most 100% of a
    /// wonder and never a negative amount of it.
    pub fn promise_share(&self) -> f64 {
        (1.0 - self.paid_fraction()) * self.feasible_fraction()
    }
}

/// [`WonderOutlook`] for `p`'s wonder under construction, or `None` when
/// there is no wonder in progress.
///
/// `resource_rate` is `effects::compute(state, p).resources` -- passed in
/// rather than recomputed, because every caller already holds a `Stats` for
/// this player and `effects::compute` is not free. Taking the one `i32` it
/// needs (rather than `&Stats`) also keeps this module free of a dependency
/// on `effects`, which it otherwise has no use for.
pub fn wonder_outlook(state: &GameState, p: &PlayerState, resource_rate: i32) -> Option<WonderOutlook> {
    if p.wonder.is_none() {
        return None;
    }
    let card = p.wonder.get();
    let stages = card.stages;
    // Clamped defensively: `wonder_steps` never exceeds `stages.len()` in a
    // legal state, but nothing here should panic if it somehow did.
    let built = (p.wonder_steps as usize).min(stages.len());
    let progress: i32 = stages[..built].iter().map(|&st| i32::from(st)).sum();
    let remaining: i32 = stages[built..].iter().map(|&st| i32::from(st)).sum();
    let n = live_count(state);
    let rounds_left = rounds_left(state, n);
    let rounds_to_antiquation = rounds_to_antiquation(state, card.age, n);

    let mut stages_left = 0.0;
    let mut turns_to_finish = 0.0;
    let mut overrun = 0.0;
    let mut age_overrun = 0.0;
    if remaining > 0 {
        stages_left = (stages.len() - built) as f64;
        let owed = f64::from(remaining) - f64::from(p.resources);
        if owed > 0.0 {
            turns_to_finish = (owed / f64::from(resource_rate).max(1.0)).min(TURNS_CAP);
            overrun = (turns_to_finish - rounds_left).max(0.0);
            age_overrun = (turns_to_finish - rounds_to_antiquation).max(0.0);
        }
    }
    Some(WonderOutlook {
        progress,
        remaining,
        stages_left,
        turns_to_finish,
        overrun,
        rounds_left,
        collect_rounds: (rounds_left - turns_to_finish).max(0.0),
        rounds_to_antiquation,
        age_overrun,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::game as G;

    /// [`CIVIL_DECK_LEN`] must equal `game::build_deck(age, true, n).len()`
    /// for every `(age, n)` this module ever indexes it with -- the guard
    /// that keeps the baked table from silently going stale if a civil
    /// card's `count` ever changes in `card_table.rs`.
    #[test]
    fn civil_deck_len_matches_build_deck_for_every_age_and_player_count() {
        // The four ages with a civil deck at all -- Age IV's is always empty
        // (`game::advance_age` empties both decks entering it), so it is
        // left out here too, matching [`CIVIL_DECK_LEN`]'s own four rows.
        let ages = [Age::A, Age::I, Age::II, Age::III];
        for (i, age) in ages.iter().copied().enumerate() {
            for n in 2..=4usize {
                let want = game::build_deck(age, true, n).len() as u32;
                assert_eq!(CIVIL_DECK_LEN[i][n - 2], want, "{age:?} at {n}p");
            }
        }
    }

    /// `tail` must not panic once the civil deck has advanced past Age III
    /// (`Age::IV` has no civil deck at all,
    /// `game::advance_age` empties both decks entering it) -- the case
    /// [`civil_deck_len_matches_build_deck_for_every_age_and_player_count`]
    /// above cannot cover, since `build_deck(Age::IV, ..)` is never called by
    /// this module's own callers either.
    #[test]
    fn tail_at_age_iv_is_zero_not_a_panic() {
        for n in 2..=4usize {
            assert_eq!(tail(n, crate::cards::Age::IV), 0);
        }
    }

    /// `rate_multiplier` at credit 0.0 is exactly 1.0 -- the "master is
    /// unaffected" byte-identity guarantee `DEFAULT_WEIGHTS["rate_horizon"]
    /// = 1.0`'s own comment leans on, checked at the point the credit is 0.0
    /// rather than 1.0.
    #[test]
    fn zero_credit_multiplier_is_exactly_one() {
        let state = G::new_game(3, 7);
        let mut w = Weights::default();
        w.set(WeightKey::RateHorizon, 0.0);
        assert_eq!(rate_multiplier(&state, &w, live_count(&state)), 1.0);
    }

    /// `lateness` is small and positive right after `new_game` -- NOT exactly
    /// 0.0, because `new_game` already deals the initial 13-card row out of
    /// the Age A civil deck before returning (confirmed against Python:
    /// `W.lateness(G.new_game(2, seed=11))` also returns `0.0855...`, not
    /// `0.0`). The docstring's "0.0 at the deal" describes the theoretical
    /// moment before anything has left the decks, which `new_game`'s return
    /// state is already just past -- so this checks "close to zero and
    /// bounded", not the unreachable exact endpoint.
    #[test]
    fn lateness_is_small_right_after_the_initial_deal() {
        for n in [2, 3, 4] {
            let state = G::new_game(n, 11);
            let lv = lateness(&state);
            assert!((0.0..0.15).contains(&lv), "{n}p: lateness {lv} out of expected range");
        }
    }

    /// `rounds_left` never returns less than 1.0, whatever the inputs --
    /// checked at the deal and is also asserted per-call by every caller's
    /// own `.max(1.0)`.
    #[test]
    fn rounds_left_is_never_below_one() {
        for n in [2, 3, 4] {
            let state = G::new_game(n, 12);
            assert!(rounds_left(&state, live_count(&state)) >= 1.0, "{n}p");
        }
    }

    /// No wonder in progress means there is no outlook to report -- `None`,
    /// not a zero-filled struct a caller could mistake for a real reading.
    #[test]
    fn a_player_with_no_wonder_in_progress_has_no_outlook_at_all() {
        let state = G::new_game(2, 20);
        assert_eq!(wonder_outlook(&state, &state.players[0], 3), None);
    }

    /// Both factors of [`WonderOutlook::earned_share`] are shares, so their
    /// product is one too: whatever the board, the evaluator can book at most
    /// 100% of a finished wonder's value and never a negative amount of it.
    /// Swept over every stage count and a range of resource incomes rather
    /// than asserted at one point, because this is the invariant that keeps
    /// the discount a DISCOUNT and not a second, unbounded multiplier.
    #[test]
    fn the_earned_share_of_a_wonder_is_always_between_none_of_it_and_all_of_it() {
        let pyramids = crate::cards::CardId::by_name("Pyramids").unwrap();
        let stages = pyramids.get().stages.len() as u8;
        for steps in 0..=stages {
            for rate in [0, 1, 4, 20] {
                for banked in [0, 5, 99] {
                    let mut state = G::new_game(2, 21);
                    state.players[0].wonder = pyramids;
                    state.players[0].wonder_steps = steps;
                    state.players[0].resources = banked;
                    let o = wonder_outlook(&state, &state.players[0], rate).expect("a wonder is in progress");
                    let share = o.earned_share();
                    assert!(
                        (0.0..=1.0).contains(&share),
                        "steps={steps} rate={rate} banked={banked}: share {share} out of [0, 1]"
                    );
                }
            }
        }
    }

    /// Paying a stage closes the resource shortfall by TWICE the stage cost
    /// -- the outstanding cost falls by it and so does the bank it is netted
    /// against -- so `turns_to_finish` strictly falls and `collect_rounds`
    /// strictly rises on exactly the move that earns it. Pinned with a
    /// deliberately slow economy (1 resource a turn) so the shortfall is
    /// large enough for the change to be visible rather than rounding away.
    #[test]
    fn paying_a_stage_buys_more_rounds_of_owning_the_finished_wonder() {
        let pyramids = crate::cards::CardId::by_name("Pyramids").unwrap();
        let outlook_at = |steps: u8| {
            let mut state = G::new_game(2, 22);
            state.players[0].wonder = pyramids;
            state.players[0].wonder_steps = steps;
            state.players[0].resources = 0;
            wonder_outlook(&state, &state.players[0], 1).expect("a wonder is in progress")
        };
        let before = outlook_at(0);
        let after = outlook_at(1);
        assert!(
            after.turns_to_finish < before.turns_to_finish,
            "turns_to_finish must fall: {} -> {}",
            before.turns_to_finish,
            after.turns_to_finish
        );
        assert!(
            after.collect_rounds > before.collect_rounds,
            "collect_rounds must rise: {} -> {}",
            before.collect_rounds,
            after.collect_rounds
        );
    }

    // ------------------------------------------------------- antiquation

    /// [`antiquated_at_end_of`] must agree with the rule `game::antiquate`
    /// actually implements, not with a remembered reading of it: at the end
    /// of age `ended`, everything with `age < ended` leaves play. Derived
    /// here by brute force (the FIRST ended-age that would cull a card of
    /// each age) and compared against the table, so a future edit to
    /// `antiquate`'s `cutoff` fails here rather than silently pricing the
    /// wrong deadline.
    ///
    /// Confirmed RED by returning `Some(Age::II)` for `Age::A`: "Age A: dies
    /// at the end of Some(I), table says Some(II)".
    #[test]
    fn a_card_survives_the_end_of_its_own_age_and_dies_at_the_end_of_the_next() {
        let ages = [Age::A, Age::I, Age::II, Age::III, Age::IV];
        for card_age in ages {
            // The first age whose ENDING would cull this card, by
            // `game::antiquate`'s own `card.age < cutoff` test.
            let culled_by = ages.iter().copied().find(|&ended| (card_age as u8) < (ended as u8));
            assert_eq!(
                antiquated_at_end_of(card_age),
                culled_by.filter(|&e| e != Age::IV || card_age != Age::III),
                "{card_age:?}: dies at the end of {culled_by:?}, table says {:?}",
                antiquated_at_end_of(card_age)
            );
        }
    }

    /// The deadline the whole change exists for: an Age A card taken in Age A
    /// runs out of time STRICTLY BEFORE the game does, and by a wide margin --
    /// the rest of the Age A deck plus the whole Age I deck, not the rest of
    /// the game. An Age III card, by contrast, has no boundary short of the
    /// game's own end, so its deadline IS `rounds_left`.
    ///
    /// Confirmed RED by having `rounds_until_antiquation` return
    /// `self.rounds_left` unconditionally: "an Age A card must run out before
    /// the game does: 12.7 vs 12.7".
    #[test]
    fn an_age_a_card_runs_out_of_time_long_before_the_game_does() {
        for n in [2usize, 3, 4] {
            let state = G::new_game(n as u8, 31);
            let rl = rounds_left(&state, n);
            let a = rounds_to_antiquation(&state, Age::A, n);
            let one = rounds_to_antiquation(&state, Age::I, n);
            let three = rounds_to_antiquation(&state, Age::III, n);
            assert!(a < rl, "{n}p: an Age A card must run out before the game does: {a} vs {rl}");
            assert!(a < one, "{n}p: Age A must run out before Age I: {a} vs {one}");
            assert!(one < rl, "{n}p: an Age I card must run out before the game does: {one} vs {rl}");
            assert_eq!(three, rl, "{n}p: an Age III card's only deadline is the game's own");
        }
    }

    /// Every deadline is inside `[0, rounds_left]` whatever the position --
    /// the property `wonder_promise`'s `feasible_fraction` divides by, swept
    /// over every age the deal can be in rather than asserted at one point.
    ///
    /// The upper clamp is load-bearing only where the two estimates are
    /// derived DIFFERENTLY, which is exactly once: `rounds_left` stops
    /// counting cards and reads `state.final_round_end` outright the moment
    /// Age IV begins, while the boundary estimate is always the undealt deck
    /// over the deal rate. The `final_round_end` leg of this sweep is
    /// therefore the one that can actually go out of range, and it is
    /// included for that reason rather than for completeness.
    #[test]
    fn no_deadline_ever_falls_outside_the_rest_of_the_game() {
        for age_civil in [Age::A, Age::I, Age::II, Age::III, Age::IV] {
            for ends_this_round in [false, true] {
                let mut state = G::new_game(2, 32);
                state.age_civil = age_civil;
                if ends_this_round {
                    state.final_round_end = Some(state.round);
                }
                let n = live_count(&state);
                let clock = AntiquationClock::at(&state, n);
                for card_age in [Age::A, Age::I, Age::II, Age::III, Age::IV] {
                    let d = clock.rounds_until_antiquation(card_age);
                    assert!(
                        (0.0..=clock.rounds_left()).contains(&d),
                        "deal in {age_civil:?} (ends_this_round={ends_this_round}), card \
                         {card_age:?}: {d} outside [0, {}]",
                        clock.rounds_left()
                    );
                }
            }
        }
    }

    /// THE MISSING BOARD FACT, as a behaviour: a wonder can be comfortably
    /// finishable before the GAME ends and still be doomed by the age
    /// boundary. `wonder_overrun` (the old coordinate) reads exactly 0.0 in
    /// that position while `wonder_age_overrun` fires -- which is what makes
    /// the new key carry information no assignment of the old one could.
    ///
    /// The fixture is the ordinary, common shape of the bug: an AGE A wonder
    /// still unfinished once the deal has moved on to Age I. It has until the
    /// end of Age I -- what is left of the deck being dealt -- while the game
    /// itself has two further ages to run.
    #[test]
    fn a_wonder_can_be_finishable_before_the_game_ends_and_still_be_doomed_by_its_age() {
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("a base-game wonder");
        let state = age_i_state_with_a_short_deck(33);
        let o = wonder_outlook(&state, &state.players[0], 1).expect("a wonder is in progress");
        assert_eq!(state.players[0].wonder, pyramids);
        assert_eq!(o.overrun, 0.0, "the OLD overrun must not fire here, got {}", o.overrun);
        assert!(
            o.age_overrun > 0.0,
            "turns_to_finish {} exceeds the antiquation deadline {}, so age_overrun {} must fire",
            o.turns_to_finish,
            o.rounds_to_antiquation,
            o.age_overrun
        );
        assert!(
            o.turns_to_finish < o.rounds_left,
            "the fixture must be finishable before the GAME ends or it proves nothing: {} vs {}",
            o.turns_to_finish,
            o.rounds_left
        );
    }

    /// A wonder that cannot be finished before its own age boundary promises
    /// nothing at all -- `feasible_fraction` is the factor that makes
    /// `wonder_promise` refuse to reach for the 66.5% of wonders the census
    /// found dying to antiquation, so it must reach exactly 0.0 and not merely
    /// get small.
    #[test]
    fn a_wonder_that_cannot_beat_its_age_boundary_promises_nothing() {
        let state = age_i_state_with_a_short_deck(34);
        // 1 resource a turn against 6 owed is past the boundary this deck is
        // about to reach.
        let doomed = wonder_outlook(&state, &state.players[0], 1).expect("a wonder is in progress");
        assert_eq!(doomed.feasible_fraction(), 0.0);
        assert_eq!(doomed.promise_share(), 0.0);
        // The same wonder with a real economy is reachable, and therefore
        // promises the whole of itself while nothing is paid yet.
        let rich = wonder_outlook(&state, &state.players[0], 9).expect("a wonder is in progress");
        assert!(rich.promise_share() > 0.0, "got {}", rich.promise_share());
        assert_eq!(rich.paid_fraction(), 0.0, "nothing is paid, so promise is at its maximum");
    }

    /// An AGE A wonder in play with the deal already into Age I and only a
    /// handful of Age I cards left: the wonder's own deadline is what remains
    /// of THIS deck (a round or two), while the game has two further ages to
    /// run. The one position where the game-end horizon and the rules'
    /// horizon disagree by an amount nothing can mistake for rounding.
    fn age_i_state_with_a_short_deck(seed: u64) -> GameState {
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("a base-game wonder");
        let mut state = G::new_game(2, seed);
        state.age_civil = Age::I;
        while state.civil_deck.len() > 8 {
            state.civil_deck.pop();
        }
        state.players[0].wonder = pyramids;
        state.players[0].wonder_steps = 0;
        state.players[0].resources = 0;
        state
    }

    /// The hand-over property `eval::DOMINATES` relies on: paying a stage
    /// moves a wonder's value OUT of `promise_share` and INTO `earned_share`,
    /// monotonically, over the same interval. Neither share may ever leave
    /// `[0, 1]` -- swept over every stage count and a range of incomes, the
    /// same way `the_earned_share_of_a_wonder_is_always_between_none_of_it_
    /// and_all_of_it` sweeps its own invariant.
    ///
    /// Confirmed RED by dropping the `(1.0 - paid_fraction())` factor from
    /// `promise_share`: "paying a stage must lower the promise: 1 -> 1".
    #[test]
    fn paying_a_stage_moves_a_wonders_value_from_its_promise_to_its_payoff() {
        let pyramids = crate::cards::CardId::by_name("Pyramids").expect("a base-game wonder");
        let stages = pyramids.get().stages.len() as u8;
        let outlook_at = |steps: u8| {
            let mut state = G::new_game(2, 35);
            state.players[0].wonder = pyramids;
            state.players[0].wonder_steps = steps;
            state.players[0].resources = 0;
            wonder_outlook(&state, &state.players[0], 6).expect("a wonder is in progress")
        };
        for steps in 0..stages {
            let before = outlook_at(steps);
            let after = outlook_at(steps + 1);
            for o in [before, after] {
                assert!((0.0..=1.0).contains(&o.promise_share()), "share {} out of [0, 1]", o.promise_share());
            }
            assert!(
                after.promise_share() < before.promise_share(),
                "paying stage {} must lower the promise: {} -> {}",
                steps + 1,
                before.promise_share(),
                after.promise_share()
            );
            assert!(
                after.earned_share() > before.earned_share(),
                "paying stage {} must raise the payoff: {} -> {}",
                steps + 1,
                before.earned_share(),
                after.earned_share()
            );
        }
    }

    /// `live_count` clamps to the 2-4 range even for a state with more seats
    /// resigned than the game normally allows to be simultaneously live.
    #[test]
    fn live_count_is_always_in_range() {
        let mut state = G::new_game(4, 13);
        state.players[2].resigned = true;
        state.players[3].resigned = true;
        assert_eq!(live_count(&state), 2);
    }

    // ----------------------------------------------------- combat_unreachable

    /// A freshly dealt game (round 1, Age A, nobody has drawn a military
    /// card -- RULES_SPEC 1.4/6.6.4) has combat structurally unreachable:
    /// every active player's military hand is empty.
    #[test]
    fn combat_unreachable_is_true_on_a_fresh_deal() {
        let state = G::new_game(2, 200);
        assert!(combat_unreachable(&state), "a fresh deal must have no military cards in any hand");
    }

    /// The gate opens the moment ANY single player -- attacker or defender,
    /// doesn't matter which -- has at least one card in a military hand,
    /// because that is the one precondition every aggression/war play
    /// shares (RULES_SPEC 5.4/5.6, "reveal" a card already in hand).
    #[test]
    fn combat_unreachable_flips_the_moment_any_one_player_has_a_military_card() {
        let mut state = G::new_game(3, 201);
        // Past the [`EARLIEST_COMBAT_ROUND`] floor, so this isolates the
        // hand-size half of the predicate rather than the round floor
        // (see the dedicated floor test below for that half).
        state.round = EARLIEST_COMBAT_ROUND;
        assert!(combat_unreachable(&state));
        // The specific card doesn't matter -- only that a hand is nonempty
        // (`combat_unreachable` reads counts, never identity; see the
        // dedicated legality test below).
        state.players[2].hand_military.push(CardId(0));
        assert!(!combat_unreachable(&state), "one player's nonempty military hand must open the gate for the whole game");
    }

    /// The [`EARLIEST_COMBAT_ROUND`] floor is a proven LOWER BOUND, not a
    /// heuristic: it must hold even when a hand is (unrealistically, for
    /// this test) already nonempty before the floor's own round -- pinning
    /// that the floor half of the predicate is checked FIRST and actually
    /// short-circuits, not merely "usually true in practice". This is the
    /// regression test for the bug this floor was added to fix: without
    /// it, `combat_unreachable` measured 47.2% military-first at 2p
    /// (STRGATE.txt section 7) because one seat's automatic end-of-turn
    /// military draw made a rival's hand nonempty mid-round, before that
    /// seat's OWN round-2 build decision.
    #[test]
    fn the_earliest_combat_round_floor_holds_even_if_a_hand_is_already_nonempty() {
        let mut state = G::new_game(2, 210);
        state.round = EARLIEST_COMBAT_ROUND - 1;
        state.players[1].hand_military.push(CardId(0));
        assert!(combat_unreachable(&state), "the floor must protect every round below EARLIEST_COMBAT_ROUND regardless of hand contents");
        state.round = EARLIEST_COMBAT_ROUND;
        assert!(!combat_unreachable(&state), "once at/past the floor, a nonempty hand must open the gate");
    }

    /// A resigned player's hand does not keep the gate closed -- they can no
    /// longer take a political action, so their leftover cards (if any) are
    /// irrelevant to whether combat is reachable. Mirrors
    /// [`GameState::active`]'s own resigned-player filter, which every
    /// other horizon computation in this module already relies on
    /// ([`live_count`] above).
    #[test]
    fn a_resigned_players_military_hand_does_not_keep_the_gate_closed() {
        let mut state = G::new_game(2, 202);
        state.players[1].hand_military.push(CardId(0));
        state.players[1].resigned = true;
        assert!(combat_unreachable(&state), "a resigned player can no longer play a political action, so their hand cannot make combat reachable");
    }

    /// LEGALITY: `combat_unreachable` may read a rival's hand SIZE (a public
    /// count -- see the function's own doc comment) but must NEVER depend on
    /// a rival's hand CONTENTS (which specific cards, which is private).
    /// Proven here by constructing two states whose rival hand sizes are
    /// identical but whose hand CONTENTS differ, and asserting the
    /// predicate returns the identical answer for both -- if it ever started
    /// reading a specific card's identity out of a rival's hand, this is the
    /// test that must catch it.
    ///
    /// Confirmed this actually catches a violation (not just tautologically
    /// green): temporarily rewrote `combat_unreachable`'s body to also
    /// require `p.hand_military.as_slice().first() != Some(&CardId(0))`
    /// -- i.e. made it peek at a specific card's IDENTITY, not just hand
    /// SIZE -- reran, watched this test fail (see STRGATE.txt section 6 for
    /// the verbatim failure), then reverted to the real, content-blind
    /// implementation below.
    #[test]
    fn combat_unreachable_does_not_depend_on_which_cards_are_in_a_hand_only_how_many() {
        let mut state_a = G::new_game(2, 203);
        let mut state_b = state_a.clone();
        // Same COUNT (one card each), deliberately different IDENTITY.
        state_a.players[1].hand_military.push(CardId(0));
        state_b.players[1].hand_military.push(CardId(1));
        assert_eq!(
            combat_unreachable(&state_a),
            combat_unreachable(&state_b),
            "the predicate must answer identically when only hand CONTENTS differ, not just hand SIZE"
        );
    }
}
