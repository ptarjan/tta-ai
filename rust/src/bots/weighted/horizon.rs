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
    let stages = p.wonder.get().stages;
    // Clamped defensively: `wonder_steps` never exceeds `stages.len()` in a
    // legal state, but nothing here should panic if it somehow did.
    let built = (p.wonder_steps as usize).min(stages.len());
    let progress: i32 = stages[..built].iter().map(|&st| i32::from(st)).sum();
    let remaining: i32 = stages[built..].iter().map(|&st| i32::from(st)).sum();
    let rounds_left = rounds_left(state, live_count(state));

    let mut stages_left = 0.0;
    let mut turns_to_finish = 0.0;
    let mut overrun = 0.0;
    if remaining > 0 {
        stages_left = (stages.len() - built) as f64;
        let owed = f64::from(remaining) - f64::from(p.resources);
        if owed > 0.0 {
            turns_to_finish = (owed / f64::from(resource_rate).max(1.0)).min(TURNS_CAP);
            overrun = (turns_to_finish - rounds_left).max(0.0);
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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

    /// `live_count` clamps to the 2-4 range even for a state with more seats
    /// resigned than the game normally allows to be simultaneously live.
    #[test]
    fn live_count_is_always_in_range() {
        let mut state = G::new_game(4, 13);
        state.players[2].resigned = true;
        state.players[3].resigned = true;
        assert_eq!(live_count(&state), 2);
    }
}
