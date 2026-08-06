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

use crate::bots::greedy::{build_bots, BotKind, Seat};
use crate::bots::weighted::weights::Weights;
use crate::game::{self, MOVE_CAP};
use crate::stats::{self, Estimate};

/// One game's result from A's point of view.
#[derive(Clone, Copy, Debug)]
pub struct Duel {
    /// 1.0 for a clean win, 1/n for an n-way tie, 0.0 otherwise.
    pub share: f64,
    pub culture_a: f64,
    /// The BEST defender's culture, not the mean. Its sign is the game result
    /// exactly, because both come from the same maximum over the same score
    /// list; the mean-of-defenders margin does NOT have that property (you can
    /// beat the average and still come third).
    pub culture_best_other: f64,
    pub moves: usize,
    pub cap_hit: bool,
}

impl Duel {
    /// A's culture minus the best defender's -- the margin
    /// `docs/LEAGUE_OBJECTIVE.md` has the league train on.
    pub fn lead(&self) -> f64 {
        self.culture_a - self.culture_best_other
    }
}

/// A duel to play: two vectors, a table, and how many games of it.
#[derive(Clone, Copy, Debug)]
pub struct Match {
    /// The challenger, seated one per game.
    pub a: Weights,
    /// The defender, seated in every other chair.
    pub b: Weights,
    /// Both sides play the same KIND; only the vectors differ. A duel across
    /// kinds is `selfplay --bots a,b`, which tallies by kind.
    pub kind: BotKind,
    pub games: usize,
    pub players: u8,
    pub seed: u64,
    pub threads: usize,
}

impl Match {
    /// A duel of the built-in vector against itself -- the shape every caller
    /// starts from and overwrites the fields it cares about.
    pub fn new(players: u8) -> Match {
        Match {
            a: Weights::defaults(),
            b: Weights::defaults(),
            kind: BotKind::Weighted,
            games: players as usize * 20,
            players,
            seed: 0,
            threads: 1,
        }
    }

    /// A's win share if the two vectors were interchangeable.
    pub fn null(&self) -> f64 {
        1.0 / self.players as f64
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

        let seats: Vec<Seat> = (0..players)
            .map(|i| Seat { kind: self.kind, weights: if i == seat { self.a } else { self.b } })
            .collect();
        let mut bots = build_bots(&seats, seed as i64);

        let mut state = game::new_game(self.players, seed);
        let outcome =
            game::play_game(&mut state, MOVE_CAP, |s, _legal| bots[s.current as usize].pick(s));

        let winners = game::winners(&state);
        let share =
            if winners.contains(&(seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 };
        let culture = |i: usize| state.players[i].culture as f64;
        let best_other =
            (0..players).filter(|i| *i != seat).map(culture).fold(f64::NEG_INFINITY, f64::max);

        Duel {
            share,
            culture_a: culture(seat),
            culture_best_other: best_other,
            moves: outcome.moves_played,
            cap_hit: outcome.move_cap_hit,
        }
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
        let mean = |f: fn(&Duel) -> f64| duels.iter().map(f).sum::<f64>() / duels.len() as f64;
        Summary {
            win: stats::paired(&shares, players),
            lead: stats::paired(&leads, players),
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
}
