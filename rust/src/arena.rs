//! Duelling two weight vectors: play the games, and say what they showed.
//!
//! This is the measurement the `arena` binary prints and the `climb` binary
//! decides on. It lives here rather than in either of them because a hill
//! climber that re-implemented "play a duel" would be free to drift from the
//! thing the report shows -- and a climb that accepts on a different
//! statistic than the one you read afterwards is the most expensive kind of
//! disagreement this project can have.
//!
//! # The design is seat-paired, and that is the whole point
//!
//! One challenger (A) sits at a table of defenders (B) and is rotated through
//! every seat: game `g` puts A in seat `g % players` and deals seed
//! `seed0 + g / players`. So every deal is played `players` times with the
//! seats swapped, and §1.9's unfair seating order -- one civil action for the
//! first seat, four for the last -- cancels exactly instead of being averaged
//! over and hoped away.
//!
//! That pairing also means the games are NOT independent samples, so the
//! interval has to cluster on the deal rather than on the game. See
//! [`crate::stats`] for why that usually makes the interval NARROWER here.
//!
//! # Why A plays B directly rather than both playing a third party
//!
//! Python's `hillclimb.challenge` measured a mutant by duelling it against a
//! field, duelling the champion against the same field on the same seeds, and
//! subtracting. That costs two games per paired sample and leaves the two
//! sides' results correlated only through the seed. Seating the mutant and
//! the champion at the SAME table on the SAME deal makes the comparison the
//! game's own result -- one game per sample, and a null of exactly
//! `1 / players` by construction rather than by argument.

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::bots::greedy::{build_bots, BotKind, Search, Seat};
use crate::bots::weighted::weights::Weights;
use crate::game::{self, MOVE_CAP};
use crate::human_policy;
use crate::stats::{self, Estimate};

/// The loader a [`BotKind`]'s weight file must go through -- [`BotKind::
/// Human`] is fit to imitate move CHOICES (`bots::human`'s doc comment) and
/// is never `dominance_repair`-ed the way a gameplay evaluator's vector is,
/// so it reads with [`human_policy::load_weights`] instead of the champion
/// [`crate::bots::weighted::eval::load_weights`] every other kind uses.
/// Exhaustive with NO wildcard arm on purpose: a future [`BotKind`] variant
/// must fail to COMPILE here until someone decides which loader it needs,
/// rather than silently falling through to the champion loader (which is
/// exactly the "a value accepted by a slot that was never meant to carry it"
/// bug this function exists to make impossible). Mirrors `bin/kindmatch.rs`'s
/// `loader_for`, which predates this one and solves the same problem for a
/// two-kind duel; this is the same routing decision for a `Match`/gauntlet
/// seat, kept here because that is where [`Seat`]-bearing callers live.
pub fn loader_for(kind: BotKind) -> fn(&std::path::Path) -> Result<Weights, String> {
    match kind {
        BotKind::Human => human_policy::load_weights,
        BotKind::Random
        | BotKind::Greedy
        | BotKind::Weighted
        | BotKind::Quiescent
        | BotKind::Plan
        | BotKind::Book
        | BotKind::Culture
        | BotKind::Military
        | BotKind::Science
        | BotKind::Wonder
        | BotKind::Infra
        | BotKind::Tempo => crate::bots::weighted::eval::load_weights,
    }
}

/// One game's result from A's point of view.
///
/// `#[non_exhaustive]` so the binaries have to go through
/// [`Duel::from_final_cultures`] rather than fill the fields in themselves.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Duel {
    /// 1.0 for a clean win, 1/n for an n-way tie, 0.0 otherwise.
    pub share: f64,
    pub culture_a: f64,
    /// The BEST defender's culture, not the mean. Its sign is the game result
    /// exactly, because both come from the same maximum over the same score
    /// list; the mean-of-defenders margin does NOT have that property (you can
    /// beat the average and still come third).
    pub culture_best_other: f64,
    /// What [`Duel::lead`] would have averaged in THIS game had A been an
    /// equally likely one of the seats: the mean over seats of that seat's own
    /// culture minus the best of the others.
    ///
    /// `lead` is a maximum over the defenders, so it is NOT centred on zero
    /// when both sides play the same vector -- the best of several equal
    /// rivals beats their average by construction. Measured over 240 games of
    /// one champion against itself: 0.0 at 2p, **-29.2** at 3p, **-43.9** at
    /// 4p. Anything treating `lead == 0` as the no-difference point is wrong
    /// above two players, which is exactly the bug this field exists to close.
    ///
    /// Under the null that every seat plays the same strength, A's seat is
    /// exchangeable with the rest, so this mean has the same expectation as
    /// `lead` itself and `lead - null_lead` has expectation zero at EVERY
    /// player count. It is computed from the game's own final scores, so it
    /// tracks how spread out this population's outcomes actually are and does
    /// not go stale as the bots improve. At 2p every term is `X_i - X_other`
    /// and they cancel exactly, so this is 0.0 and `lead - null_lead == lead`.
    pub null_lead: f64,
    pub moves: usize,
    pub cap_hit: bool,
}

impl Duel {
    /// Build a `Duel` from the final culture of EVERY seat, with A in `seat`.
    ///
    /// The only way to make one: `culture_best_other` and `null_lead` are both
    /// maxima over the other seats, so a caller filling the fields in by hand
    /// can get them out of step with each other, and every runner that plays a
    /// game already has the whole score list right there.
    pub fn from_final_cultures(
        cultures: &[f64],
        seat: usize,
        share: f64,
        moves: usize,
        cap_hit: bool,
    ) -> Duel {
        let best_of_others = |me: usize| {
            cultures
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != me)
                .map(|(_, c)| *c)
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let null_lead = cultures
            .iter()
            .enumerate()
            .map(|(i, c)| c - best_of_others(i))
            .sum::<f64>()
            / cultures.len() as f64;
        Duel {
            share,
            culture_a: cultures[seat],
            culture_best_other: best_of_others(seat),
            null_lead,
            moves,
            cap_hit,
        }
    }

    /// A's culture minus the best defender's -- the margin
    /// `docs/LEAGUE_OBJECTIVE.md` has the league train on.
    pub fn lead(&self) -> f64 {
        self.culture_a - self.culture_best_other
    }

    /// [`Duel::lead`] with this game's own no-difference point subtracted off
    /// -- see [`Duel::null_lead`]. Zero in expectation when both sides play
    /// the same vector, at every player count.
    pub fn centred_lead(&self) -> f64 {
        self.lead() - self.null_lead
    }
}

/// A duel to play: two seats, a table, and how many games of it.
///
/// Each side is a [`Seat`] -- a `BotKind` bound to the `Weights` loaded FOR
/// that kind (see [`loader_for`]) -- rather than a bare vector plus one
/// shared kind for the whole table. That used to be the design: `a: Weights,
/// b: Weights, kind: BotKind`, with a doc comment claiming "both sides play
/// the same KIND; only the vectors differ" and pointing a cross-kind duel at
/// `selfplay --bots a,b` instead. The claim was only ever true of the type
/// signature, not of what a caller could do with it -- nothing stopped
/// handing a `Human`-fit vector into that single `Weights` slot and getting
/// a silently wrong `WeightedBot` built from human-imitation numbers. Seat
/// carries its own kind so that mistake can't be assembled: every seat's
/// weights are loaded through ITS OWN kind's loader, and a same-kind table
/// (the common case) is just the special case where `a.kind == b.kind`.
#[derive(Clone, Copy, Debug)]
pub struct Match {
    /// The challenger, seated one per game.
    pub a: Seat,
    /// The defender, seated in every other chair.
    pub b: Seat,
    pub games: usize,
    pub players: u8,
    pub seed: u64,
    pub threads: usize,
}

impl Match {
    /// A duel of the built-in vector against itself -- the shape every caller
    /// starts from and overwrites the fields it cares about.
    pub fn new(players: u8) -> Match {
        let defaults = Seat { kind: BotKind::Weighted, weights: Weights::defaults(), search: Search::None };
        Match { a: defaults, b: defaults, games: players as usize * 20, players, seed: 0, threads: 1 }
    }

    /// A's win share if the two vectors were interchangeable.
    pub fn null(&self) -> f64 {
        1.0 / self.players as f64
    }

    /// The table for a game with `self.a` in `seat` and `self.b` everywhere
    /// else -- pulled out of [`Self::play_one`] so the seating itself (which
    /// (kind, weights) pair lands where) is unit-testable without playing a
    /// full game.
    fn seats_for(&self, seat: usize) -> Vec<Seat> {
        (0..self.players as usize).map(|i| if i == seat { self.a } else { self.b }).collect()
    }

    /// Reject a table this port has never been checked at, and round the game
    /// count DOWN to whole deals: a partial deal is a seat-biased observation,
    /// which is the one thing the pairing exists to exclude. Returns the
    /// rounded count so a caller can say what it actually planned.
    pub fn validate(&mut self) -> Result<usize, String> {
        if !(2..=4).contains(&self.players) {
            return Err(format!("players must be 2, 3 or 4, got {}", self.players));
        }
        if self.threads == 0 {
            return Err("threads must be at least 1".to_string());
        }
        let per_deal = self.players as usize;
        self.games -= self.games % per_deal;
        if self.games == 0 {
            return Err(format!("games must be at least {per_deal} at {}p", self.players));
        }
        Ok(self.games)
    }

    /// Game `index`: A in seat `index % players`, deal `seed + index / players`.
    pub fn play_one(&self, index: usize) -> Duel {
        let players = self.players as usize;
        let seat = index % players;
        // `* 7919 + 17` keeps consecutive deals from being consecutive seeds,
        // so neighbouring deals do not share a prefix of the shuffle.
        let seed = (self.seed.wrapping_add((index / players) as u64))
            .wrapping_mul(7919)
            .wrapping_add(17);

        let seats = self.seats_for(seat);
        let mut bots = build_bots(&seats, seed as i64);

        let mut state = game::new_game(self.players, seed);
        let outcome =
            game::play_game(&mut state, MOVE_CAP, |s, _legal| bots[s.current as usize].pick(s));

        let winners = game::winners(&state);
        let share =
            if winners.contains(&(seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 };
        let cultures: Vec<f64> = (0..players).map(|i| state.players[i].culture as f64).collect();

        Duel::from_final_cultures(
            &cultures,
            seat,
            share,
            outcome.moves_played,
            outcome.move_cap_hit,
        )
    }

    /// Play every game, in `self.threads` threads, and return them **in task
    /// order**.
    ///
    /// The order is not cosmetic the way it is in `selfplay`: [`stats::paired`]
    /// recovers a game's deal and seat from its POSITION, so a list in
    /// completion order would silently pair the wrong games together.
    pub fn play(&self) -> Vec<Duel> {
        let next = AtomicUsize::new(0);
        let done: Vec<Vec<(usize, Duel)>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..self.threads)
                .map(|_| {
                    let (next, me) = (&next, &self);
                    scope.spawn(move || {
                        let mut mine = Vec::new();
                        loop {
                            let index = next.fetch_add(1, Ordering::Relaxed);
                            if index >= me.games {
                                return mine;
                            }
                            mine.push((index, me.play_one(index)));
                        }
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
        });

        let mut slots: Vec<Option<Duel>> = vec![None; self.games];
        for (index, duel) in done.into_iter().flatten() {
            slots[index] = Some(duel);
        }
        slots.into_iter().map(|d| d.expect("every index was played")).collect()
    }
}

/// What a played duel showed. Both estimates cluster on the deal.
#[derive(Clone, Debug)]
pub struct Summary {
    pub win: Estimate,
    pub lead: Estimate,
    /// [`Duel::centred_lead`]'s estimate: the same margin with each game's own
    /// no-difference point subtracted, so its null is 0.0 at every player
    /// count. `lead`'s is not -- read this one to ask whether A actually beat
    /// B, and `lead` only for the raw culture picture.
    pub centred_lead: Estimate,
    pub mean_moves: f64,
    pub mean_culture_a: f64,
    pub mean_culture_best_other: f64,
    /// Games that ran out of moves. Always a bug when non-zero: the game ends
    /// itself, so hitting the cap means something stopped making progress.
    pub cap_hits: usize,
}

impl Summary {
    pub fn of(duels: &[Duel], players: usize) -> Summary {
        // `Some` everywhere: a game that hits the move cap is still a completed
        // game with a real winner. `stats::paired` takes `Option` because a
        // future runner may drop a game outright, and the placeholder is what
        // keeps a deal's seats recoverable by index.
        let shares: Vec<Option<f64>> = duels.iter().map(|d| Some(d.share)).collect();
        let leads: Vec<Option<f64>> = duels.iter().map(|d| Some(d.lead())).collect();
        let centred: Vec<Option<f64>> = duels.iter().map(|d| Some(d.centred_lead())).collect();
        let mean = |f: fn(&Duel) -> f64| duels.iter().map(f).sum::<f64>() / duels.len() as f64;
        Summary {
            win: stats::paired(&shares, players),
            lead: stats::paired(&leads, players),
            centred_lead: stats::paired(&centred, players),
            mean_moves: mean(|d| d.moves as f64),
            mean_culture_a: mean(|d| d.culture_a),
            mean_culture_best_other: mean(|d| d.culture_best_other),
            cap_hits: duels.iter().filter(|d| d.cap_hit).count(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// At two players the raw lead is already centred: `X_a - X_b` and
    /// `X_b - X_a` cancel, so the no-difference point is exactly zero and
    /// `centred_lead` must equal `lead` bit for bit. This is what keeps every
    /// 2p number ever measured against the raw lead comparable.
    #[test]
    fn a_two_player_duel_needs_no_centring_at_all() {
        for (a, b) in [(100.0, 100.0), (150.0, 100.0), (80.0, 137.0)] {
            let d = Duel::from_final_cultures(&[a, b], 0, 1.0, 30, false);
            assert_eq!(d.null_lead, 0.0, "2p null lead must be exactly zero, scores {a} {b}");
            assert_eq!(d.centred_lead(), d.lead());
        }
    }

    /// Above two players the best of several EQUAL rivals beats their own
    /// average, so a table of identical players shows a negative raw lead --
    /// the order-statistic artifact that made `Fitness::Margin`'s 0.5 null
    /// inoperative at 3p/4p. Whatever the table size, a dead-even game must
    /// centre to exactly zero.
    #[test]
    fn identical_scores_leave_a_negative_raw_lead_but_a_zero_centred_one() {
        for players in [3usize, 4] {
            let scores = vec![120.0; players];
            let d = Duel::from_final_cultures(&scores, 0, 1.0 / players as f64, 30, false);
            assert_eq!(d.lead(), 0.0, "{players}p: equal scores tie");
            assert_eq!(d.centred_lead(), 0.0, "{players}p: a tie is the no-difference point");
        }
        // Spread the SAME strength out and the raw lead goes negative while
        // the centred one stays at zero: A sits in seat 0, and seats 1 and 2
        // are equally likely to be the one that happened to score highest.
        let spread = [100.0, 130.0, 70.0];
        let d = Duel::from_final_cultures(&spread, 0, 0.0, 30, false);
        assert_eq!(d.lead(), -30.0, "A trails the best of the others");
        let by_seat: f64 = (0..3)
            .map(|i| Duel::from_final_cultures(&spread, i, 0.0, 30, false).centred_lead())
            .sum();
        assert!(by_seat.abs() < 1e-9, "the seats of one game must centre to zero, got {by_seat}");
    }

    /// The pairing is the design: over a whole number of deals, A must sit in
    /// every seat the same number of times.
    #[test]
    fn the_challenger_visits_every_seat_equally() {
        for players in [2usize, 3, 4] {
            let mut counts = vec![0usize; players];
            for index in 0..(players * 5) {
                counts[index % players] += 1;
            }
            assert!(counts.iter().all(|c| *c == 5), "{players}p: {counts:?}");
        }
    }

    #[test]
    fn games_round_down_to_whole_deals() {
        let mut m = Match { games: 10, ..Match::new(3) };
        assert_eq!(m.validate().unwrap(), 9);
    }

    #[test]
    fn too_few_games_for_even_one_deal_is_an_error() {
        let mut m = Match { games: 3, ..Match::new(4) };
        assert!(m.validate().is_err());
    }

    #[test]
    fn player_counts_outside_the_base_game_are_rejected() {
        assert!(Match { players: 5, ..Match::new(3) }.validate().is_err());
        assert!(Match { players: 1, ..Match::new(3) }.validate().is_err());
    }

    /// Two identical vectors must land on the null, not above it: this is the
    /// test that would catch A being handed an advantage by the harness itself
    /// rather than by its weights.
    #[test]
    fn identical_vectors_split_the_wins_evenly() {
        let m = Match { games: 9, ..Match::new(3) };
        let total: f64 = m.play().iter().map(|d| d.share).sum();
        // 3 deals, 3 seats each, A is one of three identical bots: A takes
        // exactly one share per deal because the same game is replayed with A
        // in each seat in turn.
        assert!((total - 3.0).abs() < 1e-9, "shares summed to {total}");
    }

    #[test]
    fn the_same_arguments_play_the_same_duel() {
        let m = Match { games: 6, ..Match::new(2) };
        let first: Vec<f64> = m.play().iter().map(|d| d.culture_a).collect();
        let again: Vec<f64> = m.play().iter().map(|d| d.culture_a).collect();
        assert_eq!(first, again);
    }

    /// Threading must not change the answer -- the write-back is by index for
    /// exactly this reason.
    #[test]
    fn threads_do_not_change_the_result() {
        let one = Match { games: 8, threads: 1, ..Match::new(2) }.play();
        let many = Match { games: 8, threads: 4, ..Match::new(2) }.play();
        let cultures = |ds: &[Duel]| ds.iter().map(|d| d.culture_a).collect::<Vec<_>>();
        assert_eq!(cultures(&one), cultures(&many));
    }

    // ============================================================ per-seat kind

    /// Before `Match` bound each seat's kind to its own weights, it had one
    /// `kind: BotKind` field for the WHOLE table -- every seat, A's and B's
    /// alike, played that one kind, and only the vectors ever differed. This
    /// pins that the common case the old shape existed for -- a single kind,
    /// two vectors -- still seats every player with that one kind after the
    /// rework: constructing `a` and `b` with the same `kind` must not somehow
    /// let a per-seat table drift from that.
    #[test]
    fn a_same_kind_configuration_seats_every_player_with_that_one_kind() {
        let kind = BotKind::Greedy;
        let seat = Seat { kind, weights: Weights::defaults(), search: Search::None };
        for players in [2u8, 3, 4] {
            let m = Match { a: seat, b: seat, ..Match::new(players) };
            for i in 0..players as usize {
                assert!(
                    m.seats_for(i).iter().all(|s| s.kind == kind),
                    "{players}p seat rotation {i}: not every seat was {kind:?}"
                );
            }
        }
    }

    /// The capability the per-seat rework exists to add: `a` and `b` can now
    /// be DIFFERENT kinds, and each game's table must seat the challenger's
    /// own kind in the rotating seat and the defender's own kind everywhere
    /// else -- not one kind for the whole table the way it silently would
    /// have before (a `Human`-kind `b` handed a `WeightedBot`'s evaluator, or
    /// vice versa).
    #[test]
    fn a_mixed_kind_match_seats_each_side_with_its_own_kind() {
        let a = Seat { kind: BotKind::Human, weights: Weights::defaults(), search: Search::None };
        let b = Seat { kind: BotKind::Weighted, weights: Weights::defaults(), search: Search::None };
        let m = Match { a, b, ..Match::new(3) };
        for seat in 0..3 {
            for (i, s) in m.seats_for(seat).iter().enumerate() {
                let want = if i == seat { BotKind::Human } else { BotKind::Weighted };
                assert_eq!(s.kind, want, "seat rotation {seat}, table position {i}");
            }
        }
    }

    /// `loader_for` is a total, exhaustive match with no wildcard arm, so
    /// every `BotKind` this crate defines resolves to exactly one of the two
    /// real loaders -- pins that `Human` alone gets [`human_policy::
    /// load_weights`] (no `dominance_repair`) and every other kind gets the
    /// champion loader. This is the test that would have caught the bug this
    /// module's doc comment describes: a gauntlet slot loading a human-fit
    /// vector through the champion loader and silently building a
    /// `WeightedBot` from it.
    #[test]
    fn loader_for_resolves_human_to_the_human_policy_loader_and_every_other_kind_to_the_champion_loader() {
        for &kind in BotKind::ALL {
            let got = loader_for(kind) as *const ();
            let want = if kind == BotKind::Human {
                human_policy::load_weights as *const ()
            } else {
                crate::bots::weighted::eval::load_weights as *const ()
            };
            assert_eq!(got, want, "{kind:?} resolved to the wrong loader");
        }
    }
}
