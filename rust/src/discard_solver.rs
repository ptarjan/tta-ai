//! Resolves a forced military hand-limit discard (§6.6 step 1) for
//! `rust/src/bin/replay.rs`'s human-journal reconstruction, by constraint
//! propagation over the REST of that player's journal -- see
//! `docs/REPLAY.md`'s "Military discard: solved, not given up on" section
//! for the full argument this module implements.
//!
//! # Why this is solvable at all
//!
//! BGO's journal never names which card a `"<Color> discards N card(s)"`
//! line removed -- only the count. But `replay.rs` already grounds a
//! player's military-hand card identity the instant they are later observed
//! PLAYING it (`Move::War`, `Move::Aggression`, `Move::OfferPact`,
//! `Move::PlayTactic` -- see `Replayer::ground_military_hand`), because BGO
//! DOES name the card on those lines. A card the player is later observed
//! playing was, by construction, not the card they discarded here: they
//! still had it. So among the identities `interact::discard_options`
//! currently offers for this decision, any option that reappears in a LATER
//! named play by the same player can be ruled out -- often leaving exactly
//! one candidate.
//!
//! # What this module deliberately does NOT claim
//!
//! Every option `discard_options` offers at a real decision point is,
//! itself, `replay.rs`-SIMULATED filler (see that file's module doc):
//! `Replayer::ground_military_hand` only ever grounds a card in the instant
//! before it is immediately consumed by the same move, so no card sitting
//! in hand at a discard decision ever carries a journal-verified identity of
//! its own. This module does not manufacture one. What it narrows is which
//! of the engine's own already-legal candidates is LEAST likely to
//! contradict the rest of the journal -- ruling out candidates that would
//! (via `ground_military_hand`) need to be conjured again later, which is
//! the only externally-checkable signal available. See `DiscardCertainty`
//! for how this module reports the difference between "ruled down to one"
//! and "picked one of several surviving candidates."
use std::collections::{HashMap, HashSet};

use crate::CardId;

/// One journal fact: seat `actor` is later observed playing `card` out of
/// their military hand, at journal line `lineno` (1-based, matching
/// `replay.rs`'s `Line::lineno`). Built by `replay.rs`'s
/// `prescan_future_military_needs`, which scans every `DeclareWar`/
/// `PlayAggression`/`ProposePact`/`PlayTactic` (excluding `CopyTactic`,
/// which consumes no hand card) line in the whole journal up front -- this
/// module only ever reads the slice, never re-parses text itself, keeping
/// journal parsing in one place (`corpus.rs`/`replay.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FutureNeed {
    pub lineno: usize,
    pub card: CardId,
}

/// The epistemic category one resolved discard falls into. Kept as a
/// distinct, reported-separately count rather than folded into a single
/// "discards handled" number, per this project's standing rule that a
/// completion rate built mostly on guesses is much weaker evidence than one
/// built on facts actually pinned down by the journal -- callers (and
/// `docs/REPLAY.md`) must report all three counts, never blend them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardCertainty {
    /// Exactly one of the currently offered candidates does not reappear in
    /// a later named play by this same player -- every OTHER candidate
    /// would visibly contradict a later human action, so this one is not a
    /// guess: it is the only identity consistent with the rest of the
    /// journal.
    Solved,
    /// More than one candidate survives the "not needed later" filter --
    /// genuine, irreducible ambiguity (the journal simply never names
    /// enough to distinguish them). Picked the first survivor, which is the
    /// LEAST valuable one as a defender (`interact::discard_options` sorts
    /// its output worst-first) -- mirroring a rational real player's own
    /// incentive, and the project's explicit instruction: "if there really
    /// is some ambiguity that's fine, just of the valid possibilities
    /// choose one."
    Chosen,
    /// EVERY currently offered candidate reappears in a later named play by
    /// this player -- a genuine forced collision: whichever one is picked,
    /// `ground_military_hand` will later need to conjure a fresh copy of
    /// that exact card rather than finding it already grounded, because
    /// this module removed it here. This is the "detect and report rather
    /// than silently let it pass" case the project's hard correctness rule
    /// asks for: it means EITHER this player's true hand held a card this
    /// binary never modeled at all (an under-count of the simulated hand,
    /// most likely from an earlier divergence in this game's replay), or a
    /// duplicate-named military card (some base-game military cards have
    /// more than one copy) whose second copy is what was actually
    /// discarded. Still picks the least-valuable candidate (better than
    /// refusing to progress), but every occurrence must be counted and
    /// surfaced, never silently absorbed into `Chosen`.
    ForcedCollision,
}

/// Resolves discards for one game's replay, accumulating the three
/// `DiscardCertainty` counts as it goes so the caller can report them
/// honestly at the end (`docs/REPLAY.md`'s "solved vs chosen vs forced
/// collision" table). One instance per game -- `future_needs` is specific
/// to a single journal.
pub struct DiscardSolver {
    future_needs: HashMap<u8, Vec<FutureNeed>>,
    pub solved: u32,
    pub chosen: u32,
    pub forced_collisions: u32,
}

impl DiscardSolver {
    pub fn new(future_needs: HashMap<u8, Vec<FutureNeed>>) -> Self {
        DiscardSolver { future_needs, solved: 0, chosen: 0, forced_collisions: 0 }
    }

    /// Every card `actor` is observed playing out of their own military
    /// hand at some journal line AFTER `at_lineno` -- so, every identity
    /// that provably still sits in their hand right now and must not be
    /// spent, overwritten or otherwise assumed away by anything resolving a
    /// hidden hand card at this point.
    ///
    /// Exposed (rather than left inline in [`DiscardSolver::choose`])
    /// because a forced discard is not the only place this file's replayer
    /// has to pick which of a player's SIMULATED hand cards to consume:
    /// `replay_common`'s colonization-bid grounding asks the identical
    /// question. Same predicate, one copy -- and unlike `choose`, this is a
    /// pure query and moves none of the solved/chosen/forced-collision
    /// counters, which mean "a forced DISCARD was resolved" specifically.
    pub fn needed_after(&self, actor: u8, at_lineno: usize) -> HashSet<CardId> {
        self.future_needs
            .get(&actor)
            .into_iter()
            .flatten()
            .filter(|need| need.lineno > at_lineno)
            .map(|need| need.card)
            .collect()
    }

    /// Which of `options` (the distinct card identities
    /// `interact::discard_options` currently offers `actor`, in that
    /// function's own worst-defender-first order) to discard, given the
    /// journal line number this decision is being resolved at
    /// (`at_lineno`). Returns the INDEX into `options` -- matching
    /// `Pending::Choice`'s own option list, so the caller can hand it
    /// straight to `Move::Choose` -- and which `DiscardCertainty` bucket
    /// this decision falls into. Panics if `options` is empty: callers must
    /// not call this when there is nothing to discard (the same contract
    /// `interact::push_choice`'s own callers already keep).
    pub fn choose(&mut self, actor: u8, at_lineno: usize, options: &[CardId]) -> (usize, DiscardCertainty) {
        assert!(!options.is_empty(), "DiscardSolver::choose called with no candidates to pick from");
        let needed_later = self.needed_after(actor, at_lineno);
        let safe: Vec<usize> = (0..options.len()).filter(|&i| !needed_later.contains(&options[i])).collect();
        match safe.len() {
            0 => {
                self.forced_collisions += 1;
                (0, DiscardCertainty::ForcedCollision)
            }
            1 => {
                self.solved += 1;
                (safe[0], DiscardCertainty::Solved)
            }
            _ => {
                self.chosen += 1;
                // Among several equally-legal candidates, prefer the OLDEST
                // by printed age -- never `options`' own worst-defender-first
                // order, which carries no age signal at all (a Bonus card's
                // defense value and its age are unrelated).
                //
                // This is not a guess about which specific card the real
                // player discarded (this module still does not claim that,
                // see the module doc) -- it is a claim about which CHOICE a
                // forced discard is more likely to have been. A forced
                // discard sheds exactly one card regardless of which; a card
                // one age transition away from `game::antiquate`'s own free
                // cull has zero remaining upside to keeping it over a
                // strictly newer card, so age-oldest-first is at worst
                // neutral and often strictly the rational pick -- the same
                // "burn the least valuable card" register `ground_bid_
                // ceiling`'s own doc already uses, just keyed on the one
                // piece of public information (§12.2's own age column) that
                // `discard_options`' sort does not carry at all.
                //
                // This is also where `game::antiquate`'s own wrong-COUNT
                // bug (`docs/REPLAY.md`'s "undercount cohort", game
                // `7523353` round 17) actually lives: every individual draw
                // is already tagged with its real draw-time age (`economy::
                // draw_military` only ever draws from `state.military_deck`,
                // which `game::advance_age` rebuilds to hold exactly the
                // current age's own cards), and `hand_military`'s natural
                // insertion order is therefore already oldest-first -- but
                // `options` here has been re-sorted by defense value, and
                // picking blindly off THAT order is what let a strictly
                // NEWER, safe candidate survive an ambiguous discard ahead
                // of an older one with better defense, leaving the wrong
                // card in hand for a later age transition to (wrongly) cull.
                // `min_by_key` returns the FIRST minimum on a tie, so this
                // still falls back to `options`' own worst-defender-first
                // order whenever ages match, changing nothing there.
                let &best = safe
                    .iter()
                    .min_by_key(|&&i| options[i].get().age)
                    .expect("safe is non-empty: this arm only runs when safe.len() > 1");
                (best, DiscardCertainty::Chosen)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    fn solver(needs: &[(u8, usize, CardId)]) -> DiscardSolver {
        let mut map: HashMap<u8, Vec<FutureNeed>> = HashMap::new();
        for &(actor, lineno, c) in needs {
            map.entry(actor).or_default().push(FutureNeed { lineno, card: c });
        }
        DiscardSolver::new(map)
    }

    #[test]
    fn a_candidate_never_seen_again_is_solved_when_it_is_the_only_one() {
        let warriors = card("Warriors");
        let rebellion = card("Rebellion");
        // `rebellion` is played by this same actor 5 lines later -- it was
        // still in hand, so it cannot have been the discard.
        let mut s = solver(&[(0, 100, rebellion)]);
        let (idx, cert) = s.choose(0, 50, &[warriors, rebellion]);
        assert_eq!(idx, 0);
        assert_eq!(cert, DiscardCertainty::Solved);
        assert_eq!(s.solved, 1);
        assert_eq!(s.chosen, 0);
        assert_eq!(s.forced_collisions, 0);
    }

    #[test]
    fn a_future_need_before_the_discard_line_does_not_rule_a_candidate_out() {
        // `rebellion` was played BEFORE this discard (already left the
        // hand), so it is a perfectly valid discard candidate now -- only
        // FUTURE plays are exclusionary.
        let warriors = card("Warriors");
        let rebellion = card("Rebellion");
        let mut s = solver(&[(0, 10, rebellion)]);
        let (_, cert) = s.choose(0, 50, &[warriors, rebellion]);
        // Both candidates survive (neither is needed later) -- genuine
        // ambiguity, not a solve.
        assert_eq!(cert, DiscardCertainty::Chosen);
    }

    #[test]
    fn with_no_future_needs_at_all_multiple_candidates_stay_ambiguous_and_pick_the_first() {
        let warriors = card("Warriors");
        let rebellion = card("Rebellion");
        let mut s = solver(&[]);
        let (idx, cert) = s.choose(0, 1, &[warriors, rebellion]);
        // `options` is caller-supplied in worst-defender-first order; with
        // no signal to break the tie, the first (least valuable) survives.
        assert_eq!(idx, 0);
        assert_eq!(cert, DiscardCertainty::Chosen);
        assert_eq!(s.chosen, 1);
    }

    /// `interact::discard_options`' own presentation order is worst-
    /// defender-first, which has no relationship to a card's age. Two
    /// safe (neither is ever played later by name) candidates, an Age II
    /// plain card (`defense_points` 1, the War/Aggression/Pact/Tactic/
    /// Territory/Event fallback) and an Age I Bonus card worth 2, would put
    /// the Age II card at `options[0]` under that order alone -- confirming
    /// `choose` picks the OLDER one anyway is what actually closes
    /// `docs/REPLAY.md`'s "undercount cohort" gap (game `7523353` round 17,
    /// `game::antiquate` culling the wrong COUNT because the wrong-age
    /// filler survived an earlier ambiguous discard): a forced discard
    /// sheds one card regardless of which, so the card one age transition
    /// closer to a free antiquation cull should go first.
    #[test]
    fn an_ambiguous_discard_prefers_the_older_candidate_over_options_own_worst_defender_first_order() {
        let newer_worse_defender = card("War over Territory"); // Age II, defense_points 1
        let older_better_defender = card("Military Bonus (defense 2 / colonization 1)"); // Age I, defense_points 2
        let mut buf = [CardId::NONE; crate::state::MAX_HAND];
        let n = crate::interact::discard_options(&[newer_worse_defender, older_better_defender], &mut buf);
        assert_eq!(
            buf[..n],
            [newer_worse_defender, older_better_defender],
            "sanity check: discard_options' own order puts the NEWER card first"
        );
        let mut s = solver(&[]);
        let (idx, cert) = s.choose(0, 1, &buf[..n]);
        assert_eq!(cert, DiscardCertainty::Chosen);
        assert_eq!(buf[idx], older_better_defender, "age, not defense value, must decide an ambiguous discard");
    }

    #[test]
    fn a_single_candidate_is_solved_even_with_no_future_needs() {
        let warriors = card("Warriors");
        let mut s = solver(&[]);
        let (idx, cert) = s.choose(0, 1, &[warriors]);
        assert_eq!(idx, 0);
        assert_eq!(cert, DiscardCertainty::Solved);
    }

    #[test]
    fn every_candidate_needed_later_is_a_forced_collision_not_silently_a_solve_or_a_choice() {
        let warriors = card("Warriors");
        let rebellion = card("Rebellion");
        let mut s = solver(&[(0, 100, warriors), (0, 200, rebellion)]);
        let (idx, cert) = s.choose(0, 50, &[warriors, rebellion]);
        assert_eq!(idx, 0); // still picks the least-valuable one, doesn't refuse
        assert_eq!(cert, DiscardCertainty::ForcedCollision);
        assert_eq!(s.forced_collisions, 1);
    }

    #[test]
    fn future_needs_are_scoped_per_actor_not_shared_across_players() {
        let rebellion = card("Rebellion");
        // Player 1 plays Rebellion later -- irrelevant to player 0's own
        // discard, which must not be ruled out by another seat's future.
        let mut s = solver(&[(1, 100, rebellion)]);
        let warriors = card("Warriors");
        let (_, cert) = s.choose(0, 50, &[warriors, rebellion]);
        assert_eq!(cert, DiscardCertainty::Chosen);
    }

    #[test]
    #[should_panic(expected = "no candidates")]
    fn choosing_with_no_candidates_panics_rather_than_silently_picking_nothing() {
        let mut s = solver(&[]);
        s.choose(0, 1, &[]);
    }
}
