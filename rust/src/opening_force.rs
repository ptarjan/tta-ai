//! Force a seat to open the way strong humans do (per `OPENINGS.txt`'s
//! mined categories), then hand control back to the champion evaluator
//! unchanged -- the payoff half of the "canonical openings" measurement:
//! does forcing the divergence actually win more games, or was it only ever
//! a convention?
//!
//! # The mechanism, in one sentence
//!
//! Every decision in the forced window (`state.round <= 3`, and only while
//! the relevant goal is still unmet), [`OpeningTracker::restrict`] narrows
//! the REAL legal move list down to the subset that advances the human
//! category under test, and hands that subset -- always a subset of the
//! real legal moves, so this can never manufacture an illegal move -- to
//! [`crate::bots::greedy::Bot::pick_from`], which is the SAME per-kind
//! dispatch [`crate::bots::greedy::Bot::pick`] already used; the champion's
//! own evaluator picks among the narrowed candidates exactly as it always
//! picks among the full list. If nothing in the narrowed subset is legal
//! this decision, `restrict` returns the list UNCHANGED and records a
//! fallthrough -- the champion's ordinary unconstrained choice plays instead,
//! never a manufactured or illegal one.
//!
//! # Human categories, matched exactly to `OPENINGS.txt`/`humanopenings.rs`
//!
//! - "First build kind" (`humanopenings.rs`'s `build_kind`): a
//!   `Move::Build`/`Move::Develop`/`Move::Upgrade` whose resulting card is
//!   [`crate::CardType::Mine`] (`MineFirst`) or one of the four military-unit
//!   kinds (`MilitaryFirst`).
//! - "Leader elected by round 3" (`humanopenings.rs`'s `took_leader`, driven
//!   by `Move::PlayLeader`): pursued in two steps here, since a leader must
//!   be IN HAND before it can be played -- take one from the row first (a
//!   `Move::Take` of a [`crate::CardType::Leader`] card), then play it the
//!   moment `Move::PlayLeader` is legal.

use crate::bots::greedy::Bot;
use crate::{CardType, GameState, Move};

/// Which human-opening convention (if any) a seat is forced to play. Only
/// ever consulted while `state.round <= 3` and only until its own goal is
/// satisfied -- see [`OpeningTracker::restrict`]. Exhaustively matched
/// everywhere it is inspected in this module, with NO wildcard arm, so a
/// future variant fails to compile here until someone decides what it wants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningPolicy {
    /// No forcing at all -- the champion's own unconstrained choice. Every
    /// seat that is not the one under test plays this.
    Unforced,
    /// Force the FIRST build/develop/upgrade to be Mine-kind. Humans: 60.3%
    /// of opening player-games, 54.2% win rate (n=834) -- their single most
    /// common opening line.
    MineFirst,
    /// Force a leader taken and played by the end of round 3. Humans: 83.9%
    /// (n=1161) -- their single most universal opening habit; the bot manages
    /// it only 46.2% unforced.
    LeaderByRoundThree,
    /// Both of the above, pursued independently and simultaneously.
    MineFirstAndLeader,
    /// Force the FIRST build to be a military unit -- the bot's OWN current
    /// habit (97.8% of self-play games, unforced). The mandatory control:
    /// forcing a bot to do what it already does should land at ~50% against
    /// its unforced self; if it doesn't, the harness -- not the finding -- is
    /// the bug.
    MilitaryFirst,
}

impl OpeningPolicy {
    fn wants_mine(self) -> bool {
        match self {
            OpeningPolicy::MineFirst | OpeningPolicy::MineFirstAndLeader => true,
            OpeningPolicy::Unforced | OpeningPolicy::LeaderByRoundThree | OpeningPolicy::MilitaryFirst => {
                false
            }
        }
    }

    fn wants_leader(self) -> bool {
        match self {
            OpeningPolicy::LeaderByRoundThree | OpeningPolicy::MineFirstAndLeader => true,
            OpeningPolicy::Unforced | OpeningPolicy::MineFirst | OpeningPolicy::MilitaryFirst => false,
        }
    }

    fn wants_military(self) -> bool {
        match self {
            OpeningPolicy::MilitaryFirst => true,
            OpeningPolicy::Unforced
            | OpeningPolicy::MineFirst
            | OpeningPolicy::LeaderByRoundThree
            | OpeningPolicy::MineFirstAndLeader => false,
        }
    }
}

/// The bucket `OPENINGS.txt`'s categories sort a card into. A standalone enum
/// (not reusing `CardType` directly) so the mapping from ~23 real card types
/// down to the 3 that matter here is written once, in [`bucket_of`], and
/// every caller in this module matches on the small, closed result instead
/// of re-deriving "is this one of the four military kinds" by hand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bucket {
    Mine,
    Military,
    Leader,
    Other,
}

/// Exhaustive over every [`CardType`] variant, NO wildcard arm: a new card
/// type must be sorted here deliberately, not silently land in `Other`.
fn bucket_of(kind: CardType) -> Bucket {
    match kind {
        CardType::Mine => Bucket::Mine,
        CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air => Bucket::Military,
        CardType::Leader => Bucket::Leader,
        CardType::Farm
        | CardType::Lab
        | CardType::Temple
        | CardType::Library
        | CardType::Arena
        | CardType::Theater
        | CardType::Government
        | CardType::SpecialTech
        | CardType::Wonder
        | CardType::Action
        | CardType::Tactic
        | CardType::Aggression
        | CardType::War
        | CardType::Pact
        | CardType::Bonus
        | CardType::Territory
        | CardType::Event => Bucket::Other,
    }
}

/// The card a build-like move would actually place on the board, mirroring
/// `humanopenings.rs`'s own `first_build` extraction exactly: `Build`/
/// `Develop` key on the card itself, `Upgrade` keys on `to` (the upgrade
/// target), never `from`.
fn build_target_bucket(mv: Move) -> Option<Bucket> {
    match mv {
        Move::Build { card } | Move::Develop { card } => Some(bucket_of(card.get().kind)),
        Move::Upgrade { to, .. } => Some(bucket_of(to.get().kind)),
        Move::Take { .. } | Move::WonderStep { .. } | Move::Pop | Move::PopFree | Move::Revolution { .. } | Move::PlayLeader { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => None,
    }
}

/// One goal's tally for a game: how many decisions actually got to force a
/// step toward it, and how many wanted to but had nothing legal to force
/// (see [`OpeningTracker::restrict`]'s doc comment for exactly when each
/// counts).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpeningCounters {
    pub forced: u32,
    pub fallthrough: u32,
}

/// What a decision's narrowing came to, for one goal.
enum Category {
    /// At least one legal move advances this goal -- force the choice down
    /// to exactly these.
    Forceable(Vec<Move>),
    /// Something in this goal's move FAMILY (a build, a take) is legal this
    /// decision, but none of it is this goal's target -- the "preferred move
    /// is not legal or not affordable this turn" case: fall through.
    Fallthrough,
    /// Nothing in this goal's move family is on offer this decision at all
    /// (e.g. every legal move is `Pop`/`EndTurn`) -- not a fallthrough, just
    /// not this goal's moment; leave the list untouched and say nothing.
    Irrelevant,
}

/// Per-seat, per-game state: which policy this seat is forced to play, which
/// of its goals are already satisfied, and how each goal's forcing fared.
/// One of these lives for exactly one game and is thrown away at the end --
/// no state persists across games, which is what makes seat-paired,
/// seed-shared measurement valid.
pub struct OpeningTracker {
    policy: OpeningPolicy,
    mine_built: bool,
    leader_elected: bool,
    military_built: bool,
    pub mine: OpeningCounters,
    pub leader: OpeningCounters,
    pub military: OpeningCounters,
}

impl OpeningTracker {
    pub fn new(policy: OpeningPolicy) -> OpeningTracker {
        OpeningTracker {
            policy,
            mine_built: false,
            leader_elected: false,
            military_built: false,
            mine: OpeningCounters::default(),
            leader: OpeningCounters::default(),
            military: OpeningCounters::default(),
        }
    }

    /// Whether every goal this policy carries is already satisfied (or the
    /// policy carries none) -- once true, `restrict` is a no-op for the rest
    /// of the game, exactly the "the moment the opening is complete it is
    /// Unforced" rule from the brief.
    pub fn opening_complete(&self) -> bool {
        let mine_done = !self.policy.wants_mine() || self.mine_built;
        let leader_done = !self.policy.wants_leader() || self.leader_elected;
        let military_done = !self.policy.wants_military() || self.military_built;
        mine_done && leader_done && military_done
    }

    fn build_category(&self, legal: &[Move], target: Bucket) -> Category {
        let mut matched = Vec::new();
        let mut any_build = false;
        for &mv in legal {
            if let Some(bucket) = build_target_bucket(mv) {
                any_build = true;
                if bucket == target {
                    matched.push(mv);
                }
            }
        }
        if !any_build {
            Category::Irrelevant
        } else if matched.is_empty() {
            Category::Fallthrough
        } else {
            Category::Forceable(matched)
        }
    }

    fn leader_category(&self, state: &GameState, legal: &[Move]) -> Category {
        let play_leader: Vec<Move> =
            legal.iter().copied().filter(|m| matches!(m, Move::PlayLeader { .. })).collect();
        if !play_leader.is_empty() {
            return Category::Forceable(play_leader);
        }
        let mut leader_takes = Vec::new();
        let mut any_take = false;
        for &mv in legal {
            if let Move::Take { slot } = mv {
                any_take = true;
                let card = state.card_row[slot as usize];
                if !card.is_none() && bucket_of(card.get().kind) == Bucket::Leader {
                    leader_takes.push(mv);
                }
            }
        }
        if !any_take {
            Category::Irrelevant
        } else if leader_takes.is_empty() {
            Category::Fallthrough
        } else {
            Category::Forceable(leader_takes)
        }
    }

    /// Narrow `legal` to the union of whichever unmet goals can be advanced
    /// THIS decision, or return `legal` unchanged if none can (whether
    /// because no goal applies to this decision at all, or because a goal's
    /// move family is on offer but its target isn't legal/affordable right
    /// now -- the latter bumps that goal's `fallthrough` counter).
    ///
    /// Two goals can both fire on the same decision under `MineFirstAndLeader`
    /// (a build-or-take turn can offer both a `Move::Build` and a
    /// `Move::Take` at once): their forced subsets are unioned, never
    /// intersected, because a single decision can only ever be ONE move --
    /// intersecting two disjoint move families would always be empty and
    /// silently un-force everything.
    pub fn restrict(&mut self, state: &GameState, legal: &[Move]) -> Vec<Move> {
        if state.round > 3 || self.opening_complete() {
            return legal.to_vec();
        }

        let mut forced: Vec<Move> = Vec::new();

        if self.policy.wants_mine() && !self.mine_built {
            match self.build_category(legal, Bucket::Mine) {
                Category::Forceable(mut v) => {
                    self.mine.forced += 1;
                    forced.append(&mut v);
                }
                Category::Fallthrough => self.mine.fallthrough += 1,
                Category::Irrelevant => {}
            }
        }
        if self.policy.wants_military() && !self.military_built {
            match self.build_category(legal, Bucket::Military) {
                Category::Forceable(mut v) => {
                    self.military.forced += 1;
                    forced.append(&mut v);
                }
                Category::Fallthrough => self.military.fallthrough += 1,
                Category::Irrelevant => {}
            }
        }
        if self.policy.wants_leader() && !self.leader_elected {
            match self.leader_category(state, legal) {
                Category::Forceable(mut v) => {
                    self.leader.forced += 1;
                    forced.append(&mut v);
                }
                Category::Fallthrough => self.leader.fallthrough += 1,
                Category::Irrelevant => {}
            }
        }

        if forced.is_empty() {
            legal.to_vec()
        } else {
            forced
        }
    }

    /// Update goal-satisfaction from the move actually played (whether it
    /// was forced or fell through to the champion's own choice) -- called
    /// once, right after the pick, for every decision this seat plays for
    /// the WHOLE game (not just the forced window: `pick_with_optional_force`
    /// keeps a tracker attached to its seat for as long as the seat exists).
    /// `state.round > 3` is therefore a hard no-op here, not merely an
    /// optimisation: `OPENINGS.txt`'s human categories are ALL "by round 3"
    /// (first build BY round 3, leader elected BY round 3) -- a mine the bot
    /// happens to build in round 6 during otherwise-ordinary play is real,
    /// but it is not the human-opening habit this experiment measures, and
    /// counting it would make "achieved" silently converge to ~100% for
    /// every long enough game regardless of what actually happened in the
    /// opening. Only a handful of `Move` variants matter for the match
    /// itself; the rest fall through the wildcard the same way
    /// `humanopenings.rs`'s own analogous move classifier does.
    pub fn observe(&mut self, state: &GameState, mv: Move) {
        if state.round > 3 {
            return;
        }
        match mv {
            Move::Build { card } | Move::Develop { card } => match bucket_of(card.get().kind) {
                Bucket::Mine => self.mine_built = true,
                Bucket::Military => self.military_built = true,
                Bucket::Leader | Bucket::Other => {}
            },
            Move::Upgrade { to, .. } => match bucket_of(to.get().kind) {
                Bucket::Mine => self.mine_built = true,
                Bucket::Military => self.military_built = true,
                Bucket::Leader | Bucket::Other => {}
            },
            Move::PlayLeader { .. } => self.leader_elected = true,
            Move::Take { .. } | Move::WonderStep { .. } | Move::Pop | Move::PopFree | Move::Revolution { .. } | Move::PlayAction { .. } | Move::Destroy { .. } | Move::PlayTactic { .. } | Move::CopyTactic { .. } | Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. } | Move::CancelPact { .. } | Move::PrepareEvent { .. } | Move::RemoveLeaderYellow | Move::ColumbusColonize { .. } | Move::Barbarossa { .. } | Move::BachTheater { .. } | Move::TradeFoodAsResource | Move::TradeResourceAsFood | Move::Bid { .. } | Move::BidPass | Move::Defend { .. } | Move::DefendDone | Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone | Move::Choose { .. } | Move::Churchill { .. } | Move::EndTurn | Move::PolPass | Move::Resign => {}
        }
    }

    pub fn achieved_mine(&self) -> bool {
        self.mine_built
    }

    pub fn achieved_leader(&self) -> bool {
        self.leader_elected
    }

    pub fn achieved_military(&self) -> bool {
        self.military_built
    }
}

/// Play one decision for a possibly-forced seat: narrow the legal list if
/// `tracker` is `Some`, hand the (sub)list to [`Bot::pick_from`], observe the
/// result, and return the move actually played. `tracker` is `None` for
/// every seat not under test, which just plays [`Bot::pick_from`] over the
/// unmodified legal list -- byte-for-byte what `Bot::pick` already does.
pub fn pick_with_optional_force(
    bot: &mut Bot,
    tracker: Option<&mut OpeningTracker>,
    state: &GameState,
    legal: &[Move],
) -> Move {
    match tracker {
        Some(t) => {
            let narrowed = t.restrict(state, legal);
            let mv = bot.pick_from(state, &narrowed);
            t.observe(state, mv);
            mv
        }
        None => bot.pick_from(state, legal),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::greedy::{BotKind, Search, Seat};
    use crate::bots::weighted::weights::Weights;
    use crate::game;

    fn seat() -> Seat {
        Seat { kind: BotKind::Weighted, weights: Weights::defaults(), search: Search::None }
    }

    /// `wants_mine`/`wants_leader`/`wants_military` must never overlap in a
    /// way that lets a single call union move families that were never
    /// meant to combine -- pins the exact truth table `restrict`'s doc
    /// comment describes, so a future variant added in the wrong place would
    /// break this instead of silently changing what gets unioned.
    #[test]
    fn each_policy_wants_exactly_the_goals_its_own_doc_comment_promises() {
        assert_eq!(
            (OpeningPolicy::Unforced.wants_mine(), OpeningPolicy::Unforced.wants_leader(), OpeningPolicy::Unforced.wants_military()),
            (false, false, false)
        );
        assert_eq!(
            (OpeningPolicy::MineFirst.wants_mine(), OpeningPolicy::MineFirst.wants_leader(), OpeningPolicy::MineFirst.wants_military()),
            (true, false, false)
        );
        assert_eq!(
            (
                OpeningPolicy::LeaderByRoundThree.wants_mine(),
                OpeningPolicy::LeaderByRoundThree.wants_leader(),
                OpeningPolicy::LeaderByRoundThree.wants_military()
            ),
            (false, true, false)
        );
        assert_eq!(
            (
                OpeningPolicy::MineFirstAndLeader.wants_mine(),
                OpeningPolicy::MineFirstAndLeader.wants_leader(),
                OpeningPolicy::MineFirstAndLeader.wants_military()
            ),
            (true, true, false)
        );
        assert_eq!(
            (
                OpeningPolicy::MilitaryFirst.wants_mine(),
                OpeningPolicy::MilitaryFirst.wants_leader(),
                OpeningPolicy::MilitaryFirst.wants_military()
            ),
            (false, false, true)
        );
    }

    /// `bucket_of` must sort Bronze (the printed-in-the-rulebook starting
    /// Mine tech every 2p game can build turn one once it can afford it)
    /// into `Bucket::Mine`, and Warriors (the starting military tech) into
    /// `Bucket::Military` -- if this ever drifted, `MineFirst` would stop
    /// forcing the exact card humans overwhelmingly build first.
    #[test]
    fn bronze_buckets_as_mine_and_warriors_buckets_as_military() {
        let bronze = crate::cards::CardId::by_name("Bronze").expect("Bronze must exist in the card table");
        let warriors = crate::cards::CardId::by_name("Warriors").expect("Warriors must exist in the card table");
        assert_eq!(bucket_of(bronze.get().kind), Bucket::Mine);
        assert_eq!(bucket_of(warriors.get().kind), Bucket::Military);
    }

    /// `Unforced` must never narrow anything, at any round -- it is the
    /// literal no-op every non-tested seat in the arena relies on.
    #[test]
    fn unforced_never_narrows_the_legal_list() {
        let state = game::new_game(2, 12345);
        let legal = crate::legal::legal_moves(&state);
        let mut tracker = OpeningTracker::new(OpeningPolicy::Unforced);
        let narrowed = tracker.restrict(&state, legal.as_slice());
        assert_eq!(narrowed, legal.as_slice());
    }

    /// Past round 3 the constraint must lift even if it was never satisfied
    /// -- the brief's "only through round 3" rule. Fake a round-4 state by
    /// hand (mutating just the field this function reads) rather than
    /// playing three real rounds, since only `state.round` is under test.
    #[test]
    fn forcing_lifts_after_round_3_even_when_the_goal_was_never_met() {
        let mut state = game::new_game(2, 1);
        state.round = 4;
        let legal = crate::legal::legal_moves(&state);
        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirst);
        let narrowed = tracker.restrict(&state, legal.as_slice());
        assert_eq!(narrowed, legal.as_slice(), "round > 3 must be a full pass-through");
        assert!(!tracker.achieved_mine(), "mine was never actually built in this test");
    }

    /// The core contract: while `MineFirst` is unmet and a build IS on offer
    /// this decision, `restrict` must narrow to Mine-kind builds/upgrades
    /// only -- never hand the champion a military or urban build to choose
    /// among instead.
    #[test]
    fn mine_first_narrows_a_build_decision_down_to_mine_kind_cards_only() {
        let mut state = game::new_game(2, 7);
        // §1.9: round 1 offers ONLY `Take`/`EndTurn` (`legal::action_moves`'s
        // own early return) -- no build of any kind is ever legal there, so
        // this needs round 2 to exercise a real build decision at all.
        state.round = 2;
        state.players[0].resources = 5; // afford Bronze's 2-resource build cost
        let legal = crate::legal::legal_moves(&state);
        // This decision must actually contain more than one build-like move
        // for the test to mean anything -- otherwise "narrowed to Mine" is
        // trivially true because there was nothing else to narrow away.
        let build_like =
            legal.as_slice().iter().filter(|m| build_target_bucket(**m).is_some()).count();
        assert!(build_like > 0, "fixture assumption broke: round 2 seat 0 has no build-like move at all");

        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirst);
        let narrowed = tracker.restrict(&state, legal.as_slice());
        for mv in &narrowed {
            if let Some(bucket) = build_target_bucket(*mv) {
                assert_eq!(bucket, Bucket::Mine, "a non-Mine build leaked through: {mv:?}");
            }
        }
    }

    /// If nothing in the legal list is a build/develop/upgrade at all this
    /// decision (a pure Take/Pop/EndTurn moment), `MineFirst` must leave the
    /// list untouched and must NOT count that as a fallthrough -- fallthrough
    /// means "a build was on offer and none of it was Mine", not "there was
    /// no build to be had this instant".
    #[test]
    fn mine_first_does_not_count_a_non_build_decision_as_a_fallthrough() {
        let state = game::new_game(2, 7);
        let legal = crate::legal::legal_moves(&state);
        let takes_only: Vec<Move> =
            legal.as_slice().iter().copied().filter(|m| matches!(m, Move::Take { .. })).collect();
        assert!(!takes_only.is_empty(), "fixture assumption broke: round 1 seat 0 has no Take at all");

        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirst);
        let narrowed = tracker.restrict(&state, &takes_only);
        assert_eq!(narrowed, takes_only, "no build in the list -> pass-through, not a forced narrowing");
        assert_eq!(tracker.mine.fallthrough, 0, "no build in the list -> not a fallthrough either");
        assert_eq!(tracker.mine.forced, 0);
    }

    /// A synthetic decision where a build IS legal but none of it is
    /// Mine-kind (only Warriors, the starting military tech) must fall
    /// through: the full list comes back unchanged, and the fallthrough
    /// counter -- not the forced counter -- moves.
    #[test]
    fn mine_first_falls_through_when_only_a_non_mine_build_is_legal() {
        let state = game::new_game(2, 7);
        let warriors = crate::cards::CardId::by_name("Warriors").expect("Warriors must exist");
        let synthetic = vec![Move::Build { card: warriors }, Move::EndTurn];

        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirst);
        let narrowed = tracker.restrict(&state, &synthetic);
        assert_eq!(narrowed, synthetic, "fallthrough must hand back the ORIGINAL list, not a filtered one");
        assert_eq!(tracker.mine.fallthrough, 1);
        assert_eq!(tracker.mine.forced, 0);
    }

    /// Once `observe` sees a Mine-kind build actually get played, the goal
    /// is satisfied for the rest of the game -- `restrict` must go back to a
    /// pure pass-through from that decision on, even with a military build
    /// newly on offer.
    #[test]
    fn mine_first_stops_forcing_the_instant_a_mine_is_actually_built() {
        let state = game::new_game(2, 7);
        let bronze = crate::cards::CardId::by_name("Bronze").expect("Bronze must exist");
        let warriors = crate::cards::CardId::by_name("Warriors").expect("Warriors must exist");

        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirst);
        tracker.observe(&state, Move::Build { card: bronze });
        assert!(tracker.achieved_mine());
        assert!(tracker.opening_complete());

        let synthetic = vec![Move::Build { card: warriors }, Move::EndTurn];
        let narrowed = tracker.restrict(&state, &synthetic);
        assert_eq!(narrowed, synthetic, "goal already met -> pass-through even though a military build is on offer");
        assert_eq!(tracker.mine.forced, 0, "no forcing decision was ever taken after the goal was already met");
    }

    /// `observe` must NOT count a Mine build that happens after round 3: every
    /// human category `OPENINGS.txt` mines is explicitly "by round 3", and
    /// `pick_with_optional_force` keeps calling `observe` for a seat's WHOLE
    /// game (not just the forced window), so without this gate a bot that
    /// merely builds a mine eventually -- ordinary play, round 6, nothing to
    /// do with the forced opening -- would silently count as "achieved the
    /// human opening", inflating the achieved-rate this experiment reports.
    #[test]
    fn observe_ignores_a_mine_build_that_happens_after_round_3() {
        let mut state = game::new_game(2, 7);
        state.round = 4;
        let bronze = crate::cards::CardId::by_name("Bronze").expect("Bronze must exist");

        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirst);
        tracker.observe(&state, Move::Build { card: bronze });
        assert!(!tracker.achieved_mine(), "a round-4 build must not count toward the round<=3 opening");
    }

    /// `LeaderByRoundThree`'s two-step pursuit: with a Leader card sitting in
    /// the row and affordable, `restrict` must narrow to taking IT, not any
    /// other legal Take -- pins the row-lookup half of `leader_category`
    /// (a `Take{slot}`'s cost is purely positional; only `card_row[slot]`
    /// says what it actually is).
    #[test]
    fn leader_by_round_three_narrows_a_take_decision_to_a_leader_slot_when_one_is_affordable() {
        let mut state = game::new_game(2, 7);
        // Force a Leader card into row slot 0 and make sure it's affordable:
        // slot-0 take cost is the cheapest in the row by design (§2.3), so
        // this needs no further balance changes to be legal.
        let hammurabi = crate::cards::CardId::by_name("Hammurabi").expect("Hammurabi must exist");
        assert_eq!(bucket_of(hammurabi.get().kind), Bucket::Leader);
        state.card_row[0] = hammurabi;

        let legal = crate::legal::legal_moves(&state);
        assert!(
            legal.as_slice().contains(&Move::Take { slot: 0 }),
            "fixture assumption broke: Take{{slot:0}} must be legal for this to test anything"
        );

        let mut tracker = OpeningTracker::new(OpeningPolicy::LeaderByRoundThree);
        let narrowed = tracker.restrict(&state, legal.as_slice());
        // Not exact-equal to `[Take{0}]`: the shuffled deal at this seed can
        // coincidentally place a SECOND leader elsewhere in the row too, and
        // that is legitimately also a leader-advancing move -- the contract
        // under test is "every survivor is a leader take, and slot 0 is one
        // of them", not "slot 0 is the only one".
        assert!(narrowed.contains(&Move::Take { slot: 0 }));
        for mv in &narrowed {
            let Move::Take { slot } = mv else { panic!("non-Take leaked into a leader-take narrowing: {mv:?}") };
            assert_eq!(bucket_of(state.card_row[*slot as usize].get().kind), Bucket::Leader);
        }
        assert_eq!(tracker.leader.forced, 1);
    }

    /// Once a leader is in hand, `PlayLeader` must take priority over taking
    /// a SECOND leader -- `leader_category` checks `PlayLeader` first and
    /// returns immediately when it finds one, exactly so a seat holding an
    /// already-taken leader plays it rather than hoarding more.
    #[test]
    fn leader_by_round_three_prefers_playing_an_already_held_leader_over_taking_another() {
        let mut state = game::new_game(2, 7);
        // `Move::PlayLeader` only ever appears past round 1 -- see the round-2
        // note on the previous test.
        state.round = 2;
        let hammurabi = crate::cards::CardId::by_name("Hammurabi").expect("Hammurabi must exist");
        state.players[0].hand_civil.push(hammurabi);
        state.card_row[1] = crate::cards::CardId::by_name("Aristotle").expect("Aristotle must exist");

        let legal = crate::legal::legal_moves(&state);
        assert!(
            legal.as_slice().contains(&Move::PlayLeader { card: hammurabi }),
            "fixture assumption broke: PlayLeader must be legal once a leader is in hand"
        );

        let mut tracker = OpeningTracker::new(OpeningPolicy::LeaderByRoundThree);
        let narrowed = tracker.restrict(&state, legal.as_slice());
        assert_eq!(narrowed, vec![Move::PlayLeader { card: hammurabi }]);
    }

    /// `MineFirstAndLeader` must UNION the two goals' forced subsets on a
    /// decision where both a Mine build and a leader-row Take are legal at
    /// once, rather than intersecting them down to nothing -- see
    /// `restrict`'s doc comment for why intersection would silently
    /// un-force every combined-policy decision.
    #[test]
    fn mine_first_and_leader_unions_both_goals_forced_subsets_on_the_same_decision() {
        let mut state = game::new_game(2, 7);
        // A build decision at all needs round 2 -- see the earlier round-1
        // §1.9 note.
        state.round = 2;
        state.card_row[0] = crate::cards::CardId::by_name("Hammurabi").expect("Hammurabi must exist");
        state.players[0].resources = 5; // afford Bronze's 2-resource build cost
        state.players[0].civil_actions = 4; // afford both a take and a build this turn

        let legal = crate::legal::legal_moves(&state);
        let bronze = crate::cards::CardId::by_name("Bronze").expect("Bronze must exist");
        let has_bronze_build = legal.as_slice().contains(&Move::Build { card: bronze });
        let has_leader_take = legal.as_slice().contains(&Move::Take { slot: 0 });
        assert!(has_bronze_build && has_leader_take, "fixture assumption broke: need both legal at once");

        let mut tracker = OpeningTracker::new(OpeningPolicy::MineFirstAndLeader);
        let narrowed = tracker.restrict(&state, legal.as_slice());
        assert!(narrowed.contains(&Move::Build { card: bronze }), "the Mine half must survive the union");
        assert!(narrowed.contains(&Move::Take { slot: 0 }), "the Leader half must survive the union");
        assert_eq!(tracker.mine.forced, 1);
        assert_eq!(tracker.leader.forced, 1);
    }

    /// End-to-end sanity: playing a handful of real decisions through
    /// `pick_with_optional_force` under `MilitaryFirst` (the control -- the
    /// bot's own already-dominant habit) must never crash and must always
    /// return a move the engine considers legal (`game::step` would panic
    /// otherwise) -- exercises the whole seam, not just the classifier.
    #[test]
    fn pick_with_optional_force_plays_ten_real_decisions_without_producing_an_illegal_move() {
        let mut state = game::new_game(2, 99);
        let seats = [seat(), seat()];
        let mut bots = crate::bots::greedy::build_bots(&seats, 99);
        let mut tracker = OpeningTracker::new(OpeningPolicy::MilitaryFirst);
        for _ in 0..10 {
            let legal = crate::legal::legal_moves(&state);
            let actor = state.current as usize;
            let mv = if actor == 0 {
                pick_with_optional_force(&mut bots[0], Some(&mut tracker), &state, legal.as_slice())
            } else {
                pick_with_optional_force(&mut bots[1], None, &state, legal.as_slice())
            };
            assert!(legal.as_slice().contains(&mv), "picked an illegal move: {mv:?}");
            game::step(&mut state, mv);
        }
    }
}
