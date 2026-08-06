//! Head-to-head play strength between any two [`super::spec`] contenders --
//! the ONLY promotion criterion `docs/NEURAL.md` is willing to trust.
//!
//! Ports `experiments/neural_eval.py`. That doc's load-bearing prior warning
//! is why this module exists at all rather than the loop promoting on
//! `val_pair_acc`: a value function fit by regression on outcomes "gets
//! monotonically worse as its prediction improves -- a ridge fit reached 0.81
//! held-out ranking accuracy and won **0 of 400** against the champion, and a
//! lambda ladder showed win rate 0.53 -> 0.00 as ranking accuracy rose 0.67
//! -> 0.81". Ranking accuracy is not the deliverable; an out-of-sample duel
//! with error bars is.
//!
//! # This is [`crate::arena`]'s design, generalised in exactly one direction
//!
//! `arena::Match` seats ONE weight vector against another of the SAME
//! [`crate::bots::greedy::BotKind`]. That is the right shape for the hill
//! climb, and everything about it that is a *measurement* decision is reused
//! here verbatim rather than re-derived:
//!
//!   * the seat rotation and the deal-seed scramble (`(seed + d) * 7919 +
//!     17`), so game `g` here and game `g` there are the same deal;
//!   * [`crate::arena::Duel`] as the per-game record and
//!     [`crate::arena::Summary`] as the estimator, which clusters on the
//!     DEAL -- the unit this design actually randomises.
//!
//! The one generalisation: a seat is a [`Contender`], so the two sides may be
//! different KINDS (a beam over a checkpoint against a beam over a linear
//! champion), which is precisely what both arms of the loop's gate are.
//!
//! # Why there is no shard pooling here, and no `pool_summary`
//!
//! Python fanned a gate out over N processes with disjoint `--seed0` ranges
//! and pooled the shard means, because a single-process n=200 beam-vs-beam
//! gate took an hour and CPython had no other way to use the box. That
//! fan-out is what forced `experiments/pool_summary.py` into existence, and
//! with it the whole `ci` / `ci_cluster` / `se_cluster` / `chi2` /
//! `overdispersed` vocabulary and the "do not divide `ci_cluster` by 1.96"
//! trap its comments spend forty lines warning about.
//!
//! None of that survives the port, because the reason for it does not:
//! [`Eval::play`] runs the whole match in ONE process across `threads`
//! workers, so there are no shards to pool. What replaces `se_cluster` is
//! [`crate::stats::paired`]'s `se`, clustered on the deal rather than on the
//! shard -- a strictly finer clustering of the same games (a shard contained
//! whole deals; a deal is the smallest unit the pairing makes independent),
//! computed from the games themselves rather than from six summary numbers.
//! A caller that needs "one standard error" reads [`Report::se`] and does not
//! divide anything by anything.

use crate::arena::{Duel, Summary};
use crate::game::{self, MOVE_CAP};

use super::spec::{seat_table, Contender};

/// A match to play: two contenders, a table, and how many games of it.
pub struct Eval<'a> {
    /// The challenger, seated in exactly one chair per game and rotated
    /// through every chair over a whole number of deals.
    pub a: &'a Contender,
    /// The defender, seated in every other chair.
    pub b: &'a Contender,
    pub games: usize,
    pub players: u8,
    pub seed: u64,
    pub threads: usize,
}

impl<'a> Eval<'a> {
    pub fn new(a: &'a Contender, b: &'a Contender, players: u8) -> Eval<'a> {
        Eval { a, b, games: players as usize * 20, players, seed: 0, threads: 1 }
    }

    /// A's win share if the two contenders were interchangeable.
    pub fn null(&self) -> f64 {
        1.0 / self.players as f64
    }

    /// Reject a table this port has never been checked at, and round the game
    /// count DOWN to whole deals -- a partial deal is the seat-biased
    /// observation the rotation exists to exclude. Mirrors
    /// [`crate::arena::Match::validate`] exactly; the two must not drift.
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

    /// Game `index`: A in seat `index % players`, deal `seed + index /
    /// players`. The seed arithmetic is [`crate::arena::Match::play_one`]'s,
    /// character for character, so the two drivers deal the same games.
    pub fn play_one(&self, index: usize) -> Duel {
        let players = self.players as usize;
        let seat = index % players;
        let seed = (self.seed.wrapping_add((index / players) as u64))
            .wrapping_mul(7919)
            .wrapping_add(17);

        let mut table = seat_table(self.a, self.b, self.players, seat, seed as i64);
        let mut state = game::new_game(self.players, seed);
        let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
            table[s.current as usize].pick_from(s, legal.as_slice())
        });

        let winners = game::winners(&state);
        let share = if winners.contains(&(seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 };
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

    /// Play every game across `threads` workers and return them **in task
    /// order**. The order is load-bearing, not cosmetic:
    /// [`crate::stats::paired`] recovers a game's deal from its POSITION, so
    /// a list in completion order would pair the wrong games together and
    /// report a confidently wrong interval.
    pub fn play(&self) -> Vec<Duel> {
        use std::sync::atomic::{AtomicUsize, Ordering};

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

    /// Play the match and reduce it to the numbers a caller decides on.
    pub fn run(&self) -> Report {
        let duels = self.play();
        Report::of(&duels, self.players)
    }
}

/// What a played match showed, in the form the loop's two gate arms read.
#[derive(Clone, Debug)]
pub struct Report {
    pub summary: Summary,
    pub players: u8,
}

impl Report {
    pub fn of(duels: &[Duel], players: u8) -> Report {
        Report { summary: Summary::of(duels, players as usize), players }
    }

    /// A's win share.
    pub fn win(&self) -> f64 {
        self.summary.win.mean
    }

    /// The 95% half-width, clustered on the deal. QUOTE this.
    pub fn half(&self) -> f64 {
        self.summary.win.half
    }

    /// The standard error, clustered on the deal. COMBINE this -- it is what
    /// the anchor gate's `sqrt(se_cand^2 + se_inc^2)` band is built from.
    /// Published separately from [`Report::half`] for exactly the reason
    /// `pool_summary.py` published `se_cluster` separately from `ci_cluster`:
    /// `half` already carries a `t_{k-1}` critical value, so a caller that
    /// reconstructs an SE by dividing it by 1.96 leaves `t_{k-1}/1.96`
    /// behind and gets a band that is wrong by that exact ratio.
    pub fn se(&self) -> f64 {
        self.summary.win.se
    }

    pub fn null(&self) -> f64 {
        1.0 / self.players as f64
    }

    /// The one line a shell driver parses. Every field is a bare number or
    /// the literal `NA`; nothing here is ever a plausible stand-in for a
    /// measurement that did not happen.
    ///
    /// `NA` rather than `0.0000` is the whole point, and it is not
    /// hypothetical: row 4 of the desktop's `loop2/curve.tsv` records
    /// `vs_planchamp=0.0000` for a reference match that never ran, which
    /// after the fact is indistinguishable from being beaten 0-240 by the
    /// champion. A number that parses is strictly worse than a gap, because
    /// a gap cannot be averaged into a trend.
    pub fn summary_line(&self) -> String {
        if self.summary.win.n_games == 0 {
            return "SUMMARY win=NA ci=NA se=NA a_cul=NA b_cul=NA lead=NA n=0 deals=0 capped=0"
                .to_string();
        }
        let num = |x: f64, places: usize| -> String {
            if x.is_finite() {
                format!("{x:.places$}")
            } else {
                // A single deal cannot bound itself, so `stats::paired`
                // returns an infinite half-width. Printing `inf` would let a
                // shell's numeric comparison silently succeed on it.
                "NA".to_string()
            }
        };
        format!(
            "SUMMARY win={} ci={} se={} a_cul={} b_cul={} lead={} n={} deals={} capped={}",
            num(self.summary.win.mean, 4),
            num(self.summary.win.half, 4),
            num(self.summary.win.se, 4),
            num(self.summary.mean_culture_a, 1),
            num(self.summary.mean_culture_best_other, 1),
            num(self.summary.lead.mean, 1),
            self.summary.win.n_games,
            self.summary.win.n_deals,
            self.summary.cap_hits,
        )
    }
}

// ===================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::neural::spec::Spec;

    fn contender(text: &str) -> Contender {
        Spec::parse(text).unwrap().load().unwrap()
    }

    /// The null test: two contenders built from the same spec must land on
    /// `1/players`, because the same deal is replayed with A in every seat in
    /// turn. This is the test that would catch A being handed an advantage by
    /// the harness itself rather than by its policy.
    #[test]
    fn two_identical_contenders_split_the_wins_evenly() {
        let a = contender("greedy");
        let b = contender("greedy");
        let m = Eval { games: 6, ..Eval::new(&a, &b, 2) };
        let total: f64 = m.play().iter().map(|d| d.share).sum();
        assert!((total - 3.0).abs() < 1e-9, "shares summed to {total}, expected one per deal");
    }

    #[test]
    fn threads_do_not_change_the_result() {
        let a = contender("greedy");
        let b = contender("weighted");
        let one = Eval { games: 8, threads: 1, ..Eval::new(&a, &b, 2) }.play();
        let many = Eval { games: 8, threads: 4, ..Eval::new(&a, &b, 2) }.play();
        let cultures = |ds: &[Duel]| ds.iter().map(|d| d.culture_a).collect::<Vec<_>>();
        assert_eq!(cultures(&one), cultures(&many));
    }

    #[test]
    fn the_same_arguments_play_the_same_match() {
        let a = contender("greedy");
        let b = contender("weighted");
        let m = Eval { games: 4, ..Eval::new(&a, &b, 2) };
        let first: Vec<f64> = m.play().iter().map(|d| d.culture_a).collect();
        let again: Vec<f64> = m.play().iter().map(|d| d.culture_a).collect();
        assert_eq!(first, again);
    }

    /// This driver and `arena`'s must deal the SAME games, or a result
    /// measured by one cannot be compared with a result measured by the
    /// other -- and the loop compares an anchor run against a gate run all
    /// the time.
    #[test]
    fn game_index_maps_to_the_same_deal_and_seat_as_the_arena_driver() {
        let a = contender("weighted");
        let b = contender("weighted");
        let mine = Eval { games: 6, seed: 3, ..Eval::new(&a, &b, 2) };
        let theirs = crate::arena::Match { games: 6, seed: 3, ..crate::arena::Match::new(2) };
        let culture = |d: &Duel| d.culture_a;
        assert_eq!(
            mine.play().iter().map(culture).collect::<Vec<_>>(),
            theirs.play().iter().map(culture).collect::<Vec<_>>(),
            "the two match drivers must agree game for game on identical contenders"
        );
    }

    #[test]
    fn games_round_down_to_whole_deals() {
        let a = contender("greedy");
        let b = contender("greedy");
        let mut m = Eval { games: 10, ..Eval::new(&a, &b, 3) };
        assert_eq!(m.validate().unwrap(), 9);
    }

    #[test]
    fn too_few_games_for_even_one_deal_is_an_error() {
        let a = contender("greedy");
        let b = contender("greedy");
        let mut m = Eval { games: 3, ..Eval::new(&a, &b, 4) };
        assert!(m.validate().is_err());
    }

    #[test]
    fn a_match_that_played_no_games_reports_na_rather_than_a_win_rate_of_zero() {
        let line = Report::of(&[], 2).summary_line();
        assert!(line.contains("win=NA"), "{line}");
        assert!(!line.contains("win=0"), "a missing measurement must never parse as a score: {line}");
    }

    /// A single deal cannot bound itself, so the interval is infinite --
    /// which must print as `NA`, not as `inf`, or a shell comparing it
    /// numerically would treat the gate as passed.
    #[test]
    fn an_unbounded_interval_prints_as_na_rather_than_inf() {
        let a = contender("greedy");
        let b = contender("weighted");
        let line = Eval { games: 2, ..Eval::new(&a, &b, 2) }.run().summary_line();
        assert!(line.contains("ci=NA"), "{line}");
        assert!(!line.contains("inf"), "{line}");
    }

    #[test]
    fn the_summary_line_carries_a_standard_error_so_no_caller_divides_a_half_width() {
        let a = contender("greedy");
        let b = contender("weighted");
        let line = Eval { games: 8, ..Eval::new(&a, &b, 2) }.run().summary_line();
        assert!(line.contains(" se="), "{line}");
        assert!(line.contains(" ci="), "{line}");
    }
}
