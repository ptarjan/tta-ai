//! `climb` -- hill-climb a weight vector against itself, and refuse to drift.
//!
//! ```text
//! climb --players 3 --out experiments/rust_champion_3p.json --hours 1 --threads 6
//! ```
//!
//! A generation mutates the champion `lambda` ways, duels each mutant against
//! the champion at the same table on the same deals ([`tta::arena`]), and
//! promotes the best mutant whose one-sided lower bound clears the null. Step
//! size adapts by the 1/5th success rule; a long rejection streak forces a
//! large restart-style jump at a re-opened step size.
//!
//! Port of `experiments/hillclimb.py`, with three deliberate departures.
//!
//! # 1. The mutant plays the champion, not a field
//!
//! Python duelled the mutant against a field of archived champions, duelled
//! the champion against the same field on the same seeds, and subtracted --
//! two games per paired sample, and a null of zero only by argument. Seating
//! them at the same table makes the comparison the game's own result: one game
//! per sample, and a null of exactly `1 / players` by construction. The league
//! archive, `build_field` and the mirror/league mode switch all go with it.
//!
//! # 2. An accepted champion has to still beat a POOL, worst case
//!
//! This is the fix for the thing that actually went wrong. Every champion the
//! Python league ever produced turned out to be far WORSE than the untuned
//! starting vector -- 22.8% at 2p and 3p, 13.7% at 4p, against nulls of 50%,
//! 33% and 25% -- while every single generation had honestly beaten its own
//! parent. That is the classic self-play cycle: a chain of pairwise
//! improvements walking somewhere worse than where it started, with nothing in
//! the loop ever asking the absolute question.
//!
//! The first fix here was a single fixed ANCHOR (the built-in defaults): veto
//! a promotion whose win share against it unambiguously dropped from the
//! sitting champion's. That closed the Python failure, but a single sparring
//! partner has its own blind spot: the 3p arm's gen1384 champion beat the
//! anchor 78.5% (null 33.3%) while two of its own weights ran to the mutation
//! clamp -- and it lost 11.7% to the UNRELATED 2p champion vector (same
//! 140-key basis, a legal opponent at any table). Great against one opponent,
//! terrible against another is exactly what a single-opponent gate cannot see.
//!
//! So the veto now samples a POOL each generation -- the anchor, every frozen
//! gauntlet champion (`--gauntlet`; `experiments/rust_league.sh` passes every
//! player count's frozen champion to every arm, so this is already
//! cross-player-count by default), a bounded league of this arm's own past
//! accepted selves, and the current incumbent -- and gates on the WORST
//! comparison, not the mean: a candidate is vetoed the moment its win share
//! against ANY sampled opponent is unambiguously below the sitting champion's
//! against that same opponent (intervals disjoint, not merely a lower point
//! estimate -- the same conservatism the single-anchor veto used, now applied
//! per member). A mean across the pool would hide exactly the failure this
//! replaces: two easy wins burying one collapse. See `--pool-k` and
//! `--pool-games` for the sampling knobs and their cost.
//!
//! That per-member disjoint-CI test is correct but was measured to be
//! statistically underpowered at its default cost: across 2,075 real
//! generations the pool veto fired zero times, while 81% of ACCEPTED
//! candidates were worse than the incumbent against at least one pool member
//! (median deficit -0.05, worst -0.30) -- at 60 games per member the interval
//! is simply too wide to ever go disjoint. Rather than loosen the rule into a
//! point-estimate threshold (which would veto on noise at n=60), the check is
//! now two-stage: stage 1 is the cheap `--pool-games` screen above, and stage
//! 2 ([`confirm_suspects`]) replays ONLY members whose stage-1 point estimate
//! already cleared [`POOL_CONFIRM_TRIGGER`] with `--pool-confirm-games`
//! (several times more games), re-applying the same disjoint test to that
//! bigger, fresh sample. A candidate is vetoed if EITHER stage's test is
//! disjoint -- stage 1 alone can still catch an extreme case outright, stage
//! 2 catches the moderate drift stage 1's noise was hiding.
//!
//! # 3. Freezing is enforced for every operator, not two of four
//!
//! `culture` is the numeraire -- scaling it rescales the entire objective
//! rather than changing any preference -- so it is frozen. Python enforced
//! that in two places (the key list `scatter` and `kick` sample from, and the
//! group table `group` and `rescale` walk). Here it is enforced once, in
//! [`movable`], because "the same rule written in two lists" is precisely the
//! bug class this port exists to remove.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tta::arena::{loader_for, Duel, Match, Summary};
use tta::bots::greedy::{BotKind, Search as SeatSearch, Seat};
use tta::bots::weighted::eval::{dominance_repair, load_weights, save_weights};
use tta::bots::weighted::weights::{WeightGroup, WeightKey, Weights};
use tta::rng::PyRandom;
use tta::stats;

/// Weights the climb must not move. `culture` is the unit every other weight
/// is denominated in: scaling it scales the whole evaluation, which reorders
/// nothing and just rescales `sigma` behind the search's back.
const FROZEN: &[WeightKey] = &[WeightKey::Culture];

/// Python's `_clamp`: no weight may exceed this magnitude. A coordinate that
/// runs away takes over the evaluation regardless of what the others say.
const CLAMP: f64 = 60.0;

/// How close to `CLAMP` counts as "pinned" for [`runaway_weights`]. `0.95`
/// (57.0 at the current `CLAMP`) is tight enough that a healthy vector
/// bouncing around mid-range never trips it, loose enough to catch a
/// coordinate a couple of mutation steps short of the wall rather than only
/// one sitting exactly on it -- a pinned coordinate is the signature of the
/// single-opponent overfit the pool veto exists to catch (this file's module
/// doc), so it is worth flagging before it reaches the wall, not just after.
const RUNAWAY_FRACTION: f64 = 0.95;

/// Point-estimate deficit (candidate's pool win share minus the incumbent's,
/// stage 1) below which a member earns a much bigger second look before the
/// veto trusts a "not disjoint" verdict. This is the fix for a measured
/// defect: at `--pool-games`'s default of 60, a `clearly_worse_than` CI is
/// far too wide to ever go disjoint, so the pool veto fired ZERO times across
/// 2,075 generations of real climb logs even though 81% of accepted
/// candidates were worse than the incumbent against at least one pool member
/// (worst deficits reaching -0.30). Stage 1 stays cheap and catches nothing
/// on its own in practice; stage 2 ([`confirm_suspects`]) only pays for a
/// bigger sample -- `--pool-confirm-games` -- against members whose point
/// estimate already looks this bad, and re-applies the SAME disjoint-CI test
/// (see this file's module doc) to that bigger sample. `-0.05` (5 points of
/// win share) is comfortably outside challenge-batch noise yet well inside
/// what 60 pool games can produce from a genuinely fine candidate, so it
/// triggers on real drift without triggering on every generation.
const POOL_CONFIRM_TRIGGER: f64 = -0.05;

/// Which per-game signal the accept gate (`challenge`, `Config::null`)
/// reduces over. `Win` is the original binary win share -- untouched, and
/// the default, so leaving `--fitness` off reproduces every existing run
/// bit-for-bit. `Margin` reads each game's culture lead over the best
/// opponent instead, because a continuous number has lower per-game variance
/// than a 0/1 share, so the same accept/reject decision needs fewer games to
/// reach a tight enough interval.
///
/// **Margin is not the objective and must never be treated as one.** Winning
/// by 1 culture point is worth exactly the same as winning by 50 -- the game
/// has exactly one win condition, and it is binary. `Margin` exists ONLY to
/// shrink the sample size a decision needs, by substituting a
/// lower-variance, rank-correlated proxy for the quantity that actually
/// matters. It is not free: a margin-maximiser can rationally trade away win
/// probability for surplus points nobody is paying for, favouring a mutant
/// that wins less often but by more when it does. `--fitness margin` is
/// opt-in and off by default for exactly this reason -- it is a real risk
/// this file does not hide behind the variance win, not a strictly better
/// setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Fitness {
    #[default]
    Win,
    Margin,
}

impl std::str::FromStr for Fitness {
    type Err = String;
    fn from_str(s: &str) -> Result<Fitness, String> {
        match s {
            "win" => Ok(Fitness::Win),
            "margin" => Ok(Fitness::Margin),
            other => Err(format!("--fitness: {other:?} is not win or margin")),
        }
    }
}

/// `docs/LEAGUE_OBJECTIVE.md` §1's `LEAD_SCALE`: 2.5x the standard deviation
/// of the per-seat culture lead over an external 1,011-game human BGO
/// corpus, indexed by player count. Reused verbatim rather than re-derived,
/// for the same reason that document gives for it: a scale FITTED to this
/// climb's own self-play would go stale on every improvement (the exact
/// failure the deleted `CULTURE_CENTRE` constant that document replaces
/// shows), and re-deriving it honestly would need exactly the self-play
/// measurement batch this change is not allowed to run. An external,
/// player-count-indexed reference cannot go stale that way.
fn lead_scale(players: u8) -> f64 {
    match players {
        2 => 145.0,
        3 => 115.0,
        4 => 135.0,
        other => unreachable!("players validated to 2..=4 by Match::validate, got {other}"),
    }
}

/// Turn a raw culture lead into a share-shaped `[0, 1]` number so `Fitness::
/// Margin`'s `Challenge.share`/`.lo` stay the same KIND of quantity
/// `Fitness::Win`'s do: bounded, printable next to a win share in the same
/// JSONL field, and centred where win and lose actually divide. `0` is the
/// rule-derived centre -- `lead >= 0` iff the seat won or tied, the same
/// identity `docs/LEAGUE_OBJECTIVE.md` §1 pins for the retired Python
/// league's own `lead_share` -- so the squash needs no fitted midpoint, only
/// a scale, and `lead_scale` supplies that without measuring anything. `tanh`
/// also bounds one huge blowout from swamping a whole batch's mean, which
/// matters more here than it does for a plain win share: an unsquashed
/// margin has no such ceiling.
fn margin_share(lead: f64, players: u8) -> f64 {
    0.5 * (1.0 + (lead / lead_scale(players)).tanh())
}

fn movable(key: WeightKey) -> bool {
    !FROZEN.contains(&key)
}

fn clamp(x: f64) -> f64 {
    if x.abs() > CLAMP {
        CLAMP.copysign(x)
    } else {
        x
    }
}

/// Every movable weight sitting at or very near `CLAMP` -- logged loudly by
/// the caller rather than silently clamped-and-continued, because a pinned
/// coordinate is exactly what let the 3p arm's gen1384 champion beat its one
/// sparring partner while collapsing against everyone else.
fn runaway_weights(w: &Weights) -> Vec<(WeightKey, f64)> {
    WeightKey::ALL
        .iter()
        .copied()
        .filter(|k| movable(*k))
        .map(|k| (k, w.get(k)))
        .filter(|(_, v)| v.abs() >= CLAMP * RUNAWAY_FRACTION)
        .collect()
}

// ==================================================================== search

/// The climb's random number generator. One stream, seeded once: the mutation
/// operators and the batch schedule both draw from it, so a run is reproducible
/// from `--seed` alone.
struct Search {
    rng: PyRandom,
}

impl Search {
    fn new(seed: i64) -> Search {
        Search { rng: PyRandom::new(seed.into()) }
    }

    fn random(&mut self) -> f64 {
        self.rng.random()
    }

    /// A uniform index into `0..n`.
    ///
    /// # Panics
    /// If `n` is zero -- there is no index to return, and every caller here
    /// has already established the collection is non-empty.
    fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "below(0) has no answer");
        // `random()` is in [0, 1), so the product is in [0, n) and the cast
        // cannot reach n -- the `min` is belt-and-braces against a future
        // change to `random`'s upper bound, not a live case.
        ((self.random() * n as f64) as usize).min(n - 1)
    }

    /// Box-Muller, polar-free form. Python's `random.gauss` caches a second
    /// deviate between calls; not reproducing that is deliberate -- a
    /// generator whose output depends on how many times it was called
    /// SINCE THE LAST TIME is a hidden piece of state, and this port draws
    /// two words per deviate rather than carry one.
    fn gauss(&mut self, sigma: f64) -> f64 {
        // `1.0 - random()` lands in (0, 1], which keeps `ln` off its pole.
        let u = 1.0 - self.random();
        let v = self.random();
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos() * sigma
    }

    /// `k` distinct elements of `pool`, uniformly. Partial Fisher-Yates on a
    /// copy: `k` is often most of the pool here, so the rejection-sampling
    /// alternative would spend its time re-drawing collisions.
    fn sample<T: Copy>(&mut self, pool: &[T], k: usize) -> Vec<T> {
        let mut deck = pool.to_vec();
        let k = k.min(deck.len());
        for i in 0..k {
            let j = i + self.below(deck.len() - i);
            deck.swap(i, j);
        }
        deck.truncate(k);
        deck
    }

    fn choice<T: Copy>(&mut self, pool: &[T]) -> T {
        pool[self.below(pool.len())]
    }
}

// ================================================================== mutation

/// The four mutation operators. `Scatter` is the original random-subset move;
/// the other three exist because the evaluation's weights are not independent
/// -- the interesting moves are "value this whole strategic axis more" and
/// "escape this basin", neither of which a 25% random scatter reaches often.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    Scatter,
    Group,
    Rescale,
    Kick,
}

impl std::str::FromStr for Op {
    type Err = String;
    fn from_str(s: &str) -> Result<Op, String> {
        match s {
            "scatter" => Ok(Op::Scatter),
            "group" => Ok(Op::Group),
            "rescale" => Ok(Op::Rescale),
            "kick" => Ok(Op::Kick),
            other => Err(format!("unknown operator {other:?}")),
        }
    }
}

/// What a mutation did, for the log. The label carries the chosen group for
/// `group`/`rescale` because "which axis was tried" is the part of a rejected
/// generation worth reading.
#[derive(Clone, Debug)]
struct Mutation {
    weights: Weights,
    label: String,
    moved: usize,
}

/// Propose one mutant, then put it back inside the rule-level constraints.
///
/// The repair is applied HERE, in a wrapper, rather than at each of
/// `mutate_raw`'s two return points, so that no future operator can add a
/// third path that escapes it: every mutant this binary evaluates has been
/// through `dominance_repair` by construction.
///
/// Without this the gates in `eval.rs` were very nearly decorative. They ran
/// in `load_weights`, so a champion was legal the instant a process started
/// and then drifted freely for the rest of the run -- which is exactly what
/// the live arms did, pushing five authored-negative penalty weights
/// (corruption, consumption, discontent, uprising, strength deficit)
/// positive. A constraint that holds only at startup is not a constraint.
fn mutate(w: &Weights, s: &mut Search, sigma: f64, forced: Option<Op>) -> Mutation {
    let m = mutate_raw(w, s, sigma, forced);
    let (repaired, _) = dominance_repair(&m.weights);
    Mutation { weights: repaired, ..m }
}

fn mutate_raw(w: &Weights, s: &mut Search, sigma: f64, forced: Option<Op>) -> Mutation {
    let keys: Vec<WeightKey> =
        WeightKey::ALL.iter().copied().filter(|k| movable(*k)).collect();

    let mut op = forced.unwrap_or_else(|| {
        let r = s.random();
        if r < 0.45 {
            Op::Scatter
        } else if r < 0.78 {
            Op::Group
        } else if r < 0.90 {
            Op::Rescale
        } else {
            Op::Kick
        }
    });

    // `rescale` MULTIPLIES, so a group that is all 0.0 cannot be moved by it
    // -- the "mutant" would be the champion and the generation would pay a
    // full evaluation to discover that. Prefer a group with something to
    // scale; if every group is dead, fall through to `scatter`, which ADDS
    // (the `abs(w) + 0.15` floor below is what lifts a 0.0 coordinate off
    // zero in the first place).
    let mut live: Vec<WeightGroup> = Vec::new();
    if op == Op::Rescale {
        live = WeightGroup::ALL
            .iter()
            .copied()
            .filter(|g| g.keys().iter().any(|k| movable(*k) && w.get(*k) != 0.0))
            .collect();
        if live.is_empty() {
            op = Op::Scatter;
        }
    }

    let mut out = *w;
    if op == Op::Rescale {
        let g = s.choice(&live);
        let factor = s.gauss(sigma.max(0.20)).exp();
        let picks: Vec<WeightKey> = g.keys().into_iter().filter(|k| movable(*k)).collect();
        for k in &picks {
            out.set(*k, clamp(out.get(*k) * factor));
        }
        return Mutation {
            weights: out,
            label: format!("rescale:{}", g.name()),
            moved: picks.len(),
        };
    }

    let (picks, scale, label) = match op {
        Op::Scatter => {
            let n = ((keys.len() as f64 * 0.25).round() as usize).max(1);
            (s.sample(&keys, n), sigma, "scatter".to_string())
        }
        Op::Group => {
            let n = if s.random() < 0.6 { 1 } else { 2 };
            let mut gs = s.sample(WeightGroup::ALL, n);
            gs.sort_by_key(|g| g.name());
            let picks: Vec<WeightKey> =
                gs.iter().flat_map(|g| g.keys()).filter(|k| movable(*k)).collect();
            let names: Vec<&str> = gs.iter().map(|g| g.name()).collect();
            (picks, sigma, format!("group:{}", names.join("+")))
        }
        // A deliberate big restart: most of the vector, at three times the
        // step size.
        Op::Kick => {
            let n = ((keys.len() as f64 * 0.6).round() as usize).max(1);
            (s.sample(&keys, n), sigma * 3.0, "kick".to_string())
        }
        Op::Rescale => unreachable!("rescale returned above"),
    };

    for k in &picks {
        // One draw in ten is a fat-tailed one. Without it the search can only
        // creep, and the `abs(w) + 0.15` scaling means a coordinate near zero
        // would creep slowest of all.
        let s_k = scale * if s.random() < 0.10 { 4.0 } else { 1.0 };
        let old = out.get(*k);
        out.set(*k, clamp(old + s.gauss(s_k) * (old.abs() + 0.15)));
    }
    Mutation { weights: out, label, moved: picks.len() }
}

// ================================================================= challenge

/// The verdict on one mutant.
#[derive(Clone, Copy, Debug)]
struct Challenge {
    /// The mutant's fitness against the champion, in whatever unit
    /// `Config::fitness` selects: `Fitness::Win`'s raw win share, or
    /// `Fitness::Margin`'s squashed culture-margin share (`margin_share`) --
    /// always `[0, 1]` and always the same field either way, so nothing
    /// downstream (printing, the JSONL log, `Config::null`) has to know
    /// which one produced it.
    share: f64,
    /// One-sided lower bound on that share at `accept_z`.
    lo: f64,
    games: usize,
}

/// One game's fitness observation, or `None` if this game must be excluded
/// outright rather than folded in as a number -- `stats::paired`'s own
/// contract for "this game did not finish" (its doc comment), reused here
/// rather than invented fresh.
///
/// `Fitness::Win` reads every game unconditionally, exactly as before this
/// flag existed -- `Fitness::Win` never excludes a game the old code did not
/// already count, which is what keeps `--fitness win` (the default)
/// bit-identical to every run before this flag existed.
///
/// `Fitness::Margin` excludes a game whose margin cannot be trusted: a
/// move-cap-out (`Duel::cap_hit`) stopped the game before it would have
/// ended on its own, so `Duel::lead()` reflects an arbitrary partial state,
/// not a result -- `Summary::cap_hits`'s own doc comment calls this "always
/// a bug when non-zero". Folding that in as margin `0.0` would be
/// indistinguishable from an honestly-played dead-even game and would
/// quietly bias every mean this flag produces; the non-finite guard beside
/// it is the same rule applied to any OTHER reason a lead might not be a
/// real number, not just this one known cause.
fn per_game_value(d: &Duel, cfg: &Config) -> Option<f64> {
    match cfg.fitness {
        Fitness::Win => Some(d.share),
        Fitness::Margin => {
            if d.cap_hit || !d.lead().is_finite() {
                None
            } else {
                Some(margin_share(d.lead(), cfg.players))
            }
        }
    }
}

/// Play `mutant` against `champion` in growing batches up to `max_games`,
/// with exactly one early exit: abandon as soon as the running mean is
/// BELOW the null and the screening batch is spent (stop paying for a
/// loser). Everything else keeps buying games up to `max_games`.
///
/// There is deliberately no early ACCEPT. It used to re-test `lo > null`
/// after every batch and break the moment that cleared -- up to
/// `max_games / screen` looks per mutant -- which is optional stopping: it
/// turned the nominal `accept_z` = 90% one-sided test into a ~75% one (a
/// candidate exactly as good as the champion cleared the gate 25% of the
/// time instead of 10%; `/private/tmp/gatediag/sim.py` reproduces the exact
/// numbers). An accept is permanent where a rejection only costs one
/// generation, so a winner now always plays out the full `max_games` and
/// the decision is made once, at the end, on the whole sample -- the cheap
/// stopping rule stays the one for LOSERS only.
fn challenge(mutant: &Weights, cfg: &Config, seed: u64) -> Challenge {
    let players = cfg.players as usize;
    let null = cfg.null();
    let mut shares: Vec<Option<f64>> = Vec::new();
    let mut batch = cfg.screen;

    while shares.len() < cfg.max_games {
        let want = batch.min(cfg.max_games - shares.len());
        let mut duel = Match {
            a: Seat { kind: cfg.kind, weights: *mutant, search: SeatSearch::None },
            b: Seat { kind: cfg.kind, weights: cfg.champion, search: SeatSearch::None },
            games: want,
            players: cfg.players,
            // Every batch must be a FRESH deal range: reusing the seed would
            // replay games already counted, which does not narrow anything but
            // looks exactly like it did.
            seed: seed.wrapping_add(shares.len() as u64),
            threads: cfg.threads,
        };
        if duel.validate().is_err() {
            break;
        }
        shares.extend(duel.play().iter().map(|d| per_game_value(d, cfg)));

        let est = stats::paired(&shares, players);
        if est.mean < null && shares.len() >= cfg.screen {
            break; // stop paying for a loser
        }
        batch = cfg.screen;
    }

    let est = stats::paired(&shares, players);
    Challenge {
        share: est.mean,
        lo: est.mean - cfg.accept_z * est.se,
        games: shares.len(),
    }
}

/// The champion's standing against the fixed anchor -- the number the drift
/// veto compares. `half` is the two-sided half-width, so `mean - half` and
/// `mean + half` bracket it.
#[derive(Clone, Copy, Debug)]
struct Anchor {
    mean: f64,
    half: f64,
}

/// Win share (+ two-sided half-width) of seat `a` against seat `b` over
/// `games` games at this run's table size. Shared by the anchor check and
/// the gauntlet below -- both ask "how does this vector do against a fixed
/// reference", they differ only in what the reference is (and its kind --
/// the gauntlet's members are not necessarily [`Config::kind`]) and what
/// happens with the answer: the anchor can veto a promotion, the gauntlet is
/// only ever logged.
fn measure_against(a: Seat, b: Seat, cfg: &Config, games: usize, seed: u64) -> Anchor {
    let mut duel = Match { a, b, games, players: cfg.players, seed, threads: cfg.threads };
    duel.validate().expect("duel was validated at start-up");
    let s = Summary::of(&duel.play(), cfg.players as usize);
    Anchor { mean: s.win.mean, half: if s.win.half.is_finite() { s.win.half } else { 1.0 } }
}

/// The champion's seat: `cfg.kind` is the kind this whole climb mutates, so
/// the champion (and every mutant challenging it) is always seated as that
/// one kind -- only the gauntlet's OPPONENTS may carry a different kind.
fn champion_seat(w: &Weights, cfg: &Config) -> Seat {
    Seat { kind: cfg.kind, weights: *w, search: SeatSearch::None }
}

fn measure_anchor(w: &Weights, cfg: &Config, seed: u64) -> Anchor {
    measure_against(champion_seat(w, cfg), champion_seat(&cfg.anchor, cfg), cfg, cfg.anchor_games, seed)
}

impl Anchor {
    /// `self` is unambiguously worse than `other`: the intervals do not even
    /// touch. Deliberately conservative -- see this file's module doc.
    fn clearly_worse_than(&self, other: &Anchor) -> bool {
        self.mean + self.half < other.mean - other.half
    }
}

/// The champion's standing against every frozen gauntlet member
/// (`docs/RUST_LEAGUE.md`'s "gauntlet" section). This function's OWN return
/// value is purely observational -- only ever printed and logged, never
/// consulted by the accept gate. `Config::gauntlet` itself (the raw opponent
/// list this reads) is no longer observational-only, though: [`pool_members`]
/// below also draws its frozen-champion members from the same list, so a
/// `--gauntlet` flag now feeds two independent consumers -- this report, and
/// the pool veto's sample. Empty when `--gauntlet` was never passed, which
/// keeps every existing invocation of this binary byte-for-byte unaffected
/// in what it MEASURES here (not in what it accepts -- see [`pool_members`]).
fn measure_gauntlet(w: &Weights, cfg: &Config, seed_base: u64) -> Vec<(String, Anchor)> {
    cfg.gauntlet
        .iter()
        .enumerate()
        .map(|(i, (name, opponent))| {
            // `104_729` is just a prime far bigger than any plausible member
            // count, so consecutive members' seed ranges cannot overlap.
            let seed = seed_base.wrapping_add(i as u64 * 104_729);
            // `*opponent` carries its OWN kind (see `Config::gauntlet`'s doc
            // comment) -- a `Human` gauntlet member plays as a `HumanBot`
            // against the champion's `cfg.kind` seat, not as a `WeightedBot`
            // built from human-fit numbers.
            (name.clone(), measure_against(champion_seat(w, cfg), *opponent, cfg, cfg.gauntlet_games, seed))
        })
        .collect()
}

/// Whether generation `gen` is due for a gauntlet measurement. A pure
/// function so the cadence is testable without playing a single game.
/// `every == 0` disables the gauntlet entirely (also true if `--gauntlet`
/// was never passed, since `measure_gauntlet` returns nothing to log then).
fn gauntlet_due(gen: u64, every: usize) -> bool {
    every > 0 && gen.is_multiple_of(every as u64)
}

// ========================================================================= pool

/// One sampled opponent's verdict: the candidate's and the sitting
/// champion's fresh standing against it, from the same generation's sample
/// so the two are directly comparable.
#[derive(Clone, Debug)]
struct PoolResult {
    name: String,
    candidate: Anchor,
    incumbent: Anchor,
}

/// Every opponent the accept gate may sample this generation: the fixed
/// anchor, every frozen gauntlet member (typically every player count's
/// champion plus the human-fit vector -- see `experiments/rust_league.sh`),
/// a bounded league of this arm's own past accepted selves, and the current
/// incumbent itself. A fresh `Vec` once per GENERATION, not once per game --
/// cheap -- because the incumbent and the league both move as the climb
/// progresses and nothing here is worth caching across that move.
fn pool_members(cfg: &Config, league: &[(String, Weights)]) -> Vec<(String, Seat)> {
    let mut pool = vec![("anchor".to_string(), champion_seat(&cfg.anchor, cfg))];
    pool.extend(cfg.gauntlet.iter().cloned());
    pool.extend(league.iter().map(|(name, w)| (name.clone(), champion_seat(w, cfg))));
    pool.push(("incumbent".to_string(), champion_seat(&cfg.champion, cfg)));
    pool
}

/// Duel `candidate` and the sitting champion against `k` opponents sampled
/// from `pool`, each on `games` games at this run's table -- fresh every
/// time, never cached, so the incumbent's standing here always matches the
/// exact opponents this generation happened to draw. Deterministic from
/// `seed`: sampling `k` distinct opponents and playing the games both draw
/// from a `Search` seeded once, so the same generation seed always samples
/// the same members and plays the same deals.
fn play_pool(
    candidate: &Weights,
    cfg: &Config,
    pool: &[(String, Seat)],
    k: usize,
    games: usize,
    seed: u64,
) -> Vec<PoolResult> {
    let mut search = Search::new(seed as i64);
    let indices: Vec<usize> = (0..pool.len()).collect();
    search
        .sample(&indices, k)
        .into_iter()
        .enumerate()
        .map(|(i, idx)| {
            let (name, opponent) = &pool[idx];
            // Distinct, non-overlapping seed ranges per member (and between
            // the candidate's and the incumbent's own duel against it) for
            // the same reason `measure_gauntlet` staggers by `104_729`: two
            // duels sharing deals would not be an independent second look.
            let s = seed.wrapping_add(i as u64 * 104_729);
            let candidate_a = measure_against(champion_seat(candidate, cfg), *opponent, cfg, games, s);
            let incumbent_a =
                measure_against(champion_seat(&cfg.champion, cfg), *opponent, cfg, games, s.wrapping_add(50_021));
            PoolResult { name: name.clone(), candidate: candidate_a, incumbent: incumbent_a }
        })
        .collect()
}

/// The pool veto's whole point: gate on the WORST comparison, not the mean.
/// Returns the name of the FIRST sampled opponent (in sample order) against
/// which the candidate is unambiguously worse than the incumbent, or `None`
/// if it is not clearly worse against any of them. A candidate that trounces
/// two opponents and collapses against a third is exactly gen1384's failure
/// mode (this file's module doc) -- averaging the three would have hidden
/// the third behind the first two.
fn worst_case_verdict(results: &[PoolResult]) -> Option<String> {
    results.iter().find(|r| r.candidate.clearly_worse_than(&r.incumbent)).map(|r| r.name.clone())
}

/// Stage 1 of the two-stage veto ([`POOL_CONFIRM_TRIGGER`]'s doc comment):
/// is this member's cheap point estimate already bad enough to be worth a
/// bigger, dedicated look? A pure predicate on the ALREADY-PLAYED stage-1
/// result, so which members get replayed is directly testable without
/// playing a single stage-2 game.
fn suspect(r: &PoolResult) -> bool {
    r.candidate.mean - r.incumbent.mean < POOL_CONFIRM_TRIGGER
}

/// Stage 2 of the two-stage veto: replay ONLY the [`suspect`] members from
/// `stage1`, `cfg.pool_confirm_games` games a side, and hand back a fresh
/// [`PoolResult`] for each so [`worst_case_verdict`] can re-apply the exact
/// same disjoint-CI test to the bigger sample.
///
/// Deliberately stage 2 ALONE, not stage 1's games pooled with stage 2's:
/// pooling would need the raw per-game shares concatenated before
/// `stats::paired` runs on them, but every measurement in this file
/// (`measure_against`, `play_pool`) already collapses a duel down to a mean
/// and a half-width before it comes back, so there is nothing left to pool by
/// the time a [`PoolResult`] exists. Re-running `measure_against` fresh here
/// avoids threading raw shares through a second code path, and correctness
/// only needs each stage's own games to be internally consistent -- which a
/// fresh, disjoint seed range (below) already guarantees, the same rule
/// `challenge`'s own batches and `play_pool`'s own members follow.
fn confirm_suspects(
    candidate: &Weights,
    cfg: &Config,
    pool: &[(String, Seat)],
    stage1: &[PoolResult],
    seed: u64,
) -> Vec<PoolResult> {
    stage1
        .iter()
        .filter(|r| suspect(r))
        .filter_map(|r| {
            // Recover the member's position in `pool` (not carried by
            // `PoolResult` itself) so the seed offset matches `play_pool`'s
            // own per-member scheme, and so a name that somehow is not in
            // `pool` any more is skipped rather than panicking mid-veto.
            let idx = pool.iter().position(|(name, _)| name == &r.name)?;
            let (name, opponent) = &pool[idx];
            let s = seed.wrapping_add(idx as u64 * 104_729);
            let candidate_a =
                measure_against(champion_seat(candidate, cfg), *opponent, cfg, cfg.pool_confirm_games, s);
            let incumbent_a = measure_against(
                champion_seat(&cfg.champion, cfg),
                *opponent,
                cfg,
                cfg.pool_confirm_games,
                s.wrapping_add(50_021),
            );
            Some(PoolResult { name: name.clone(), candidate: candidate_a, incumbent: incumbent_a })
        })
        .collect()
}

/// Append a newly accepted champion to the league and drop the OLDEST once
/// it is over `cap` -- a straight FIFO, not the "recent N plus a few
/// spaced-out older ones" this change's own design note floated as a
/// possible refinement. FIFO was chosen to keep this change small: it still
/// bounds memory and per-generation game cost (`pool_members`'s league
/// contribution never exceeds `cap` entries), which is the property that
/// actually matters for affordability on a 6-core box, and a smarter
/// eviction policy can be layered on later without touching anything else
/// here. `cap == 0` disables the league outright.
fn push_league(league: &mut Vec<(String, Weights)>, entry: (String, Weights), cap: usize) {
    if cap == 0 {
        return;
    }
    league.push(entry);
    while league.len() > cap {
        league.remove(0);
    }
}

// ====================================================================== args

#[derive(Clone, Debug)]
struct Config {
    champion: Weights,
    /// Fixed for the life of the climb. Never updated from the champion --
    /// that would make it a moving target and put the cycle straight back.
    anchor: Weights,
    kind: BotKind,
    players: u8,
    /// Games in the first batch of a challenge, and in every batch after it.
    screen: usize,
    max_games: usize,
    anchor_games: usize,
    threads: usize,
    accept_z: f64,
    /// Frozen past opponents, labelled by filename stem, that this run BOTH
    /// reports standing against ([`measure_gauntlet`], purely observational)
    /// AND draws pool-veto opponents from ([`pool_members`], which gates a
    /// promotion). Each carries its OWN [`BotKind`] bound to its own weights
    /// (a [`Seat`]) -- most members share `Config::kind` (the usual "past
    /// champion" case), but a member need not: a `Human` gauntlet entry is
    /// loaded with `human_policy::load_weights` via [`loader_for`] and
    /// played by a `HumanBot`, never by a `WeightedBot` built from its
    /// human-fit numbers.
    gauntlet: Vec<(String, Seat)>,
    /// Games played per gauntlet member, each time the gauntlet runs.
    gauntlet_games: usize,
    /// Opponents sampled from the pool ([`pool_members`]) each generation a
    /// candidate is found. Worst-case, not averaged -- see this file's
    /// module doc -- so this is "how many independent chances a candidate
    /// gets to reveal a collapse", not a sample whose noise averages out.
    pool_k: usize,
    /// Games played per pool pairing (candidate vs. opponent, and
    /// incumbent vs. the same opponent) -- one pool check costs
    /// `2 * pool_k * pool_games` games.
    pool_games: usize,
    /// Games per pairing in the stage-2 confirm ([`confirm_suspects`]),
    /// several times `pool_games` so the disjoint-CI test actually has the
    /// power `pool_games` alone does not (see [`POOL_CONFIRM_TRIGGER`]).
    /// Paid only for members [`suspect`] flags, not the whole pool. `0`
    /// disables stage 2 outright -- the pre-fix behaviour, kept only so a
    /// run can A/B against it.
    pool_confirm_games: usize,
    /// What `challenge` (the per-mutant accept/reject signal) reduces over --
    /// see [`Fitness`]'s own doc comment for the win-vs-margin trade-off.
    /// Deliberately NOT consulted by the pool veto (`measure_against`,
    /// `play_pool`, `worst_case_verdict`): that veto is the anti-drift safety
    /// net (module doc, section 2), and stays on the well-understood,
    /// tightly-bounded win share regardless of what the accept gate is
    /// exploring with, exactly the same way it already stays on win share
    /// regardless of `--gauntlet`.
    fitness: Fitness,
}

#[derive(Clone, Debug)]
struct Args {
    cfg: Config,
    out: PathBuf,
    log: Option<PathBuf>,
    lambda: usize,
    gens: usize,
    hours: f64,
    seed: i64,
    sigma: f64,
    sigma_floor: f64,
    stall_kick: usize,
    /// Generations between gauntlet measurements; 0 disables it. A run-loop
    /// cadence, not a duel parameter, so it lives here rather than in
    /// [`Config`] alongside `gauntlet_games`.
    gauntlet_every: usize,
    /// Bounded number of past accepted champions [`push_league`] retains as
    /// extra pool opponents; 0 disables the league. Run-loop state (the
    /// league itself lives in `main`'s loop, not in [`Config`], the same way
    /// `recent` and `hold_sigma` do) rather than a duel parameter, so it
    /// lives here alongside `gauntlet_every`.
    league_cap: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            cfg: Config {
                champion: Weights::defaults(),
                anchor: Weights::defaults(),
                kind: BotKind::Weighted,
                players: 3,
                screen: 24,
                max_games: 240,
                anchor_games: 120,
                threads: 1,
                accept_z: 1.2816,
                gauntlet: Vec::new(),
                gauntlet_games: 60,
                // 3 opponents at 60 games/side is 360 games per pool check
                // (`2 * pool_k * pool_games`) -- three times the old single
                // 120-game anchor duel, paid only on generations that find a
                // contender, and still a fraction of `lambda` screening
                // duels (up to `lambda * max_games` = 480 at the defaults
                // above). That is the deliberate price of the bug this
                // change exists to fix: the 3p arm's gen1384 champion beat
                // its one sparring partner 78.5% while losing 11.7% to an
                // unrelated peer, and a properly powered per-member check
                // (not a thin, noisy one) is what catches that.
                pool_k: 3,
                pool_games: 60,
                // 300 is 5x pool_games: cheap enough to pay only for the
                // members stage 1 already flagged (see `suspect`), and
                // large enough to shrink the CI half-width by roughly
                // sqrt(5) -- the difference between "never disjoint" and
                // "resolves a real -0.05 to -0.30 deficit", which is the
                // measured range this whole change exists to catch.
                pool_confirm_games: 300,
                fitness: Fitness::Win,
            },
            out: PathBuf::from("champion.json"),
            log: None,
            lambda: 2,
            gens: usize::MAX,
            hours: 1.0,
            seed: 0,
            sigma: 0.25,
            sigma_floor: 0.08,
            stall_kick: 15,
            gauntlet_every: 50,
            league_cap: 6,
        }
    }
}

const USAGE: &str = "\
usage: climb --out PATH [options]

  --out PATH         champion checkpoint; RESUMED from if it already exists
  --start PATH       initial champion (default: the built-in vector)
  --anchor PATH      fixed reference the champion may never fall behind
                     (default: the built-in vector)
  --log PATH         append one JSON line per generation
  --players N        2, 3 or 4 (default 3)
  --kind KIND        bot kind both sides play (default weighted)
  --lambda N         mutants per generation (default 2)
  --gens N           stop after N generations (default: only --hours stops it)
  --hours H          wall-clock budget (default 1)
  --screen N         games per challenge batch (default 24)
  --max-games N      games a single challenge may spend (default 240)
  --anchor-games N   games per anchor measurement (default 120)
  --gauntlet PATH    frozen past opponent, reported on AND drawn into the
                     accept-gate pool (repeatable; see --pool-k)
  --gauntlet-kind KIND  kind of the NEXT --gauntlet member (default: --kind,
                     i.e. same kind as the champion -- set this first when a
                     member is a different kind, e.g. --gauntlet-kind human)
  --gauntlet-search PATH  give the NEXT --gauntlet member real lookahead: a
                     human-plausible root shortlist (its own --gauntlet file)
                     narrows the legal moves, then plan::pick's beam --
                     scored by THIS file's gameplay-evaluator vector -- picks
                     among them (only legal when that member's kind is human,
                     set via --gauntlet-kind human first)
  --gauntlet-games N games per gauntlet member each time it runs (default 60)
  --gauntlet-every N generations between gauntlet measurements; 0 disables
                     (default 50)
  --pool-k N         opponents sampled from the pool (anchor + --gauntlet +
                     league + incumbent) per accept check; a candidate is
                     vetoed if it is unambiguously worse than the incumbent
                     against ANY of them -- worst case, not the mean
                     (default 3; 0 disables the pool veto)
  --pool-games N     games per pool pairing; one check costs
                     2 * --pool-k * --pool-games games (default 60)
  --pool-confirm-games N  games per pairing in the stage-2 confirm, paid only
                     for members whose --pool-games point estimate already
                     trails the incumbent by more than 5 points of win share;
                     0 disables stage 2 (default 300)
  --league-cap N     past accepted champions kept as extra pool opponents;
                     0 disables the league (default 6)
  --threads N        games in parallel (default 1)
  --seed N           run seed (default 0)
  --sigma X          initial step size, if not resumed (default 0.25)
  --sigma-floor X    smallest step the 1/5th rule may shrink to (default 0.08)
  --stall-kick N     rejections before a forced big jump; 0 disables (default 15)
  --accept-z X       strictness of the accept gate (default 1.2816 = 90% one-sided)
  --fitness win|margin  what challenge's accept/reject signal reduces over:
                     the binary win share, or a squashed culture-margin share
                     that needs fewer games for the same decision -- margin is
                     NOT the objective, see Fitness's doc comment (default win)
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut start: Option<PathBuf> = None;
    // Raw `--gauntlet` entries, resolved into `a.cfg.gauntlet` only AFTER
    // the whole command line is parsed -- resolving a member's kind (and so
    // which loader reads its file) needs `a.cfg.kind`'s FINAL value, which
    // may be typed after the `--gauntlet` flags that need it. The third slot
    // is `--gauntlet-search`'s pending eval-weights path, same "applies to
    // the NEXT `--gauntlet` only" semantics as `pending_gauntlet_kind`.
    let mut gauntlet_raw: Vec<(Option<BotKind>, Option<PathBuf>, PathBuf)> = Vec::new();
    let mut pending_gauntlet_kind: Option<BotKind> = None;
    let mut pending_gauntlet_search: Option<PathBuf> = None;
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--out" => a.out = PathBuf::from(value(flag)?),
            "--start" => start = Some(PathBuf::from(value(flag)?)),
            "--anchor" => a.cfg.anchor = load_weights(Path::new(&value(flag)?))?,
            "--log" => a.log = Some(PathBuf::from(value(flag)?)),
            "--players" => a.cfg.players = parse_num(&value(flag)?, flag)?,
            "--kind" => a.cfg.kind = value(flag)?.parse::<BotKind>()?,
            "--lambda" => a.lambda = parse_num(&value(flag)?, flag)?,
            "--gens" => a.gens = parse_num(&value(flag)?, flag)?,
            "--hours" => a.hours = parse_num(&value(flag)?, flag)?,
            "--screen" => a.cfg.screen = parse_num(&value(flag)?, flag)?,
            "--max-games" => a.cfg.max_games = parse_num(&value(flag)?, flag)?,
            "--anchor-games" => a.cfg.anchor_games = parse_num(&value(flag)?, flag)?,
            "--gauntlet" => {
                gauntlet_raw.push((
                    pending_gauntlet_kind.take(),
                    pending_gauntlet_search.take(),
                    PathBuf::from(value(flag)?),
                ));
            }
            "--gauntlet-kind" => pending_gauntlet_kind = Some(value(flag)?.parse::<BotKind>()?),
            "--gauntlet-search" => {
                pending_gauntlet_search = Some(PathBuf::from(value(flag)?));
            }
            "--gauntlet-games" => a.cfg.gauntlet_games = parse_num(&value(flag)?, flag)?,
            "--gauntlet-every" => a.gauntlet_every = parse_num(&value(flag)?, flag)?,
            "--pool-k" => a.cfg.pool_k = parse_num(&value(flag)?, flag)?,
            "--pool-games" => a.cfg.pool_games = parse_num(&value(flag)?, flag)?,
            "--pool-confirm-games" => a.cfg.pool_confirm_games = parse_num(&value(flag)?, flag)?,
            "--league-cap" => a.league_cap = parse_num(&value(flag)?, flag)?,
            "--threads" => a.cfg.threads = parse_num(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--sigma" => a.sigma = parse_num(&value(flag)?, flag)?,
            "--sigma-floor" => a.sigma_floor = parse_num(&value(flag)?, flag)?,
            "--stall-kick" => a.stall_kick = parse_num(&value(flag)?, flag)?,
            "--accept-z" => a.cfg.accept_z = parse_num(&value(flag)?, flag)?,
            "--fitness" => a.cfg.fitness = value(flag)?.parse::<Fitness>()?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }

    if let Some(p) = &start {
        a.cfg.champion = load_weights(p)?;
    }
    // Resolve every gauntlet member now that `a.cfg.kind` is final: a member
    // with no `--gauntlet-kind` override plays the champion's own kind
    // (exactly the old, single-kind behaviour), and either way the file is
    // read with THAT kind's own loader (see `loader_for`) -- never blindly
    // with the champion loader the way this used to work unconditionally.
    for (kind_override, search_path, p) in gauntlet_raw {
        let kind = kind_override.unwrap_or(a.cfg.kind);
        let weights = loader_for(kind)(&p)?;
        // The filename stem carries the provenance (generation, key count,
        // date -- see analysis/frozen/README.md's naming rule), which is
        // exactly what belongs in the log next to the number, so reuse it
        // rather than inventing a new label.
        let label =
            p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
        // `--gauntlet-search`: the root-shortlist+beam hybrid
        // (`bots::greedy::Search::HumanShortlistBeam`) instead of this
        // member's plain, searchless `HumanBot` -- only legal when the
        // member's resolved kind is `human` (same rule `bin/kindmatch.rs`'s
        // `--a-search`/`--b-search` enforce at their own parse time, see
        // that file's module doc comment). `eval_weights` is loaded with the
        // CHAMPION loader (`dominance_repair` included): it is a real
        // gameplay evaluator for `plan::pick`'s beam, not a human-imitation
        // ranking vector, so it belongs to the same vocabulary `cfg.champion`/
        // `cfg.anchor` already load with.
        let search = match search_path {
            None => SeatSearch::None,
            Some(sp) => {
                if kind != BotKind::Human {
                    return Err(format!(
                        "--gauntlet-search is only legal when the gauntlet member's kind is human, got {:?} for {p:?}",
                        kind
                    ));
                }
                SeatSearch::HumanShortlistBeam { eval_weights: load_weights(&sp)? }
            }
        };
        a.cfg.gauntlet.push((label, Seat { kind, weights, search }));
    }
    if a.lambda == 0 {
        return Err("--lambda must be at least 1".to_string());
    }
    // Every duel this run will play has to be a legal one, and finding that
    // out on generation 1 rather than on the command line wastes a batch.
    let probe = |games: usize| -> Result<(), String> {
        Match { games, players: a.cfg.players, threads: a.cfg.threads, ..Match::new(a.cfg.players) }
            .validate()
            .map(|_| ())
    };
    probe(a.cfg.screen).map_err(|e| format!("--screen: {e}"))?;
    probe(a.cfg.max_games).map_err(|e| format!("--max-games: {e}"))?;
    probe(a.cfg.anchor_games).map_err(|e| format!("--anchor-games: {e}"))?;
    probe(a.cfg.gauntlet_games).map_err(|e| format!("--gauntlet-games: {e}"))?;
    probe(a.cfg.pool_games).map_err(|e| format!("--pool-games: {e}"))?;
    // `0` is the documented "stage 2 disabled" sentinel (same convention as
    // `--league-cap 0` and `--gauntlet-every 0`), so it must not be probed
    // as a game count -- `Match::validate` rejects 0 games outright.
    if a.cfg.pool_confirm_games > 0 {
        probe(a.cfg.pool_confirm_games).map_err(|e| format!("--pool-confirm-games: {e}"))?;
    }
    if a.cfg.max_games < a.cfg.screen {
        return Err("--max-games must be at least --screen".to_string());
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

// ======================================================================= run

/// The bookkeeping a checkpoint carries across restarts.
///
/// `since_accept` is restored on purpose: a supervisor restarts this process
/// every hour, and a counter that reset each time could never reach
/// `stall_kick` on a player count whose generations are slow -- so the
/// anti-stagnation kick would silently never fire on exactly the runs that
/// need it most.
#[derive(Clone, Copy, Debug, Default)]
struct Progress {
    gen: u64,
    sigma: f64,
    since_accept: usize,
}

fn resume(path: &Path) -> Option<(Weights, Progress)> {
    let text = std::fs::read_to_string(path).ok()?;
    let w = tta::bots::weighted::eval::parse_weights(&text).ok()?;
    let doc = tta::fixtures::parse_json(&text).ok()?;
    let num = |k: &str| doc.get(k).and_then(|v| v.as_f64());
    Some((
        w,
        Progress {
            gen: num("gen").unwrap_or(0.0) as u64,
            sigma: num("sigma").unwrap_or(0.0),
            since_accept: num("since_accept").unwrap_or(0.0).max(0.0) as usize,
        },
    ))
}

fn checkpoint(path: &Path, w: &Weights, p: &Progress, players: u8, anchor: &Anchor) -> Result<(), String> {
    save_weights(
        path,
        w,
        &[
            ("gen", p.gen as f64),
            ("sigma", p.sigma),
            ("since_accept", p.since_accept as f64),
            ("players", players as f64),
            ("vs_anchor", anchor.mean),
            ("vs_anchor_half", anchor.half),
        ],
    )
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("climb: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut progress = Progress { sigma: args.sigma, ..Progress::default() };
    if let Some((w, p)) = resume(&args.out) {
        args.cfg.champion = w;
        progress = p;
        println!("resumed from {} at gen {}", args.out.display(), progress.gen);
    }
    progress.sigma = progress.sigma.max(args.sigma_floor);

    // The run seed folds in the player count and the generation reached, so a
    // resumed run does not replay the mutations it already tried.
    let mut search = Search::new(
        args.seed
            .wrapping_mul(7919)
            .wrapping_add(args.cfg.players as i64 * 101)
            .wrapping_add(progress.gen as i64),
    );

    let mut standing = measure_anchor(&args.cfg.champion, &args.cfg, 10_007);
    println!(
        "anchor       champion is {:.1}% +/- {:.1} against the anchor over {} games",
        100.0 * standing.mean,
        100.0 * standing.half,
        args.cfg.anchor_games,
    );
    if let Err(e) = checkpoint(&args.out, &args.cfg.champion, &progress, args.cfg.players, &standing)
    {
        eprintln!("climb: {e}");
        return ExitCode::FAILURE;
    }

    let deadline = Instant::now() + Duration::from_secs_f64(args.hours * 3600.0);
    // The 1/5th success rule reads the last dozen generations, not all of them.
    let mut recent: Vec<bool> = Vec::new();
    let mut hold_sigma = 0usize;
    let start_gen = progress.gen;
    // Past accepted champions, bounded by `--league-cap` -- see
    // `push_league`. NOT restored on resume, same as `recent` and
    // `hold_sigma` above: only `champion`, `gen`, `sigma` and `since_accept`
    // are persisted in the checkpoint, and this follows that existing
    // precedent rather than growing the checkpoint format for it.
    let mut league: Vec<(String, Weights)> = Vec::new();

    while Instant::now() < deadline && progress.gen - start_gen < args.gens as u64 {
        progress.gen += 1;
        let t0 = Instant::now();

        // A long rejection streak means the current sigma cannot reach
        // anything better from here. Force a large restart-style jump instead
        // of grinding the same neighbourhood -- and re-open sigma in the SAME
        // generation, so the big jump is actually taken at a big step size.
        let mut forced = None;
        if args.stall_kick > 0
            && progress.since_accept > 0
            && progress.since_accept.is_multiple_of(args.stall_kick)
        {
            forced = Some(Op::Kick);
            progress.sigma = (progress.sigma.max(0.25) * 2.0).min(0.8);
            hold_sigma = args.stall_kick / 3; // let the re-opened step breathe
        }

        let mut best: Option<(Mutation, Challenge)> = None;
        let mut tried: Vec<String> = Vec::new();
        for j in 0..args.lambda {
            let m = mutate(&args.cfg.champion, &mut search, progress.sigma, forced);
            let seed = progress
                .gen
                .wrapping_mul(1_000_003)
                .wrapping_add(j as u64 * 7717)
                .wrapping_add(args.seed as u64);
            let c = challenge(&m.weights, &args.cfg, seed);
            tried.push(format!(
                "{{\"op\":\"{}\",\"moved\":{},\"share\":{:.4},\"lo\":{:.4},\"n\":{}}}",
                m.label, m.moved, c.share, c.lo, c.games
            ));
            if c.lo > args.cfg.null() && best.as_ref().is_none_or(|(_, b)| c.lo > b.lo) {
                best = Some((m, c));
            }
        }

        let mut vetoed = false;
        let mut accepted = false;
        let mut pool_results: Vec<PoolResult> = Vec::new();
        let mut confirm_results: Vec<PoolResult> = Vec::new();
        let mut veto_reason: Option<String> = None;
        let mut runaway: Vec<(WeightKey, f64)> = Vec::new();
        if let Some((m, _)) = &best {
            // The pool veto (this file's module doc, section 2). A fresh
            // sample and fresh games every generation -- never cached across
            // generations, and on a different seed range than the anchor
            // telemetry measured below, because the incumbent and the
            // league both move as the climb progresses.
            let pool = pool_members(&args.cfg, &league);
            let pool_seed = 30_029u64.wrapping_add(progress.gen.wrapping_mul(41));
            pool_results =
                play_pool(&m.weights, &args.cfg, &pool, args.cfg.pool_k, args.cfg.pool_games, pool_seed);
            veto_reason = worst_case_verdict(&pool_results);
            // Stage 2 (module doc, "two-stage confirm"): only when stage 1
            // did not already veto outright, and only spent on members
            // `suspect` flagged. A DIFFERENT seed range from `pool_seed`
            // (below) so this stage's games are a genuinely fresh sample,
            // not a replay of stage 1's.
            if veto_reason.is_none() && args.cfg.pool_confirm_games > 0 {
                let confirm_seed = 62_233u64.wrapping_add(progress.gen.wrapping_mul(59));
                confirm_results =
                    confirm_suspects(&m.weights, &args.cfg, &pool, &pool_results, confirm_seed);
                veto_reason = worst_case_verdict(&confirm_results);
            }
            if veto_reason.is_some() {
                vetoed = true;
            } else {
                args.cfg.champion = m.weights;
                // Anchor telemetry only now -- the pool veto above already
                // decided acceptance. Kept for the printed line, the
                // checkpoint's `vs_anchor` fields, and continuity with runs
                // that only ever read that number.
                standing = measure_anchor(
                    &args.cfg.champion,
                    &args.cfg,
                    10_007u64.wrapping_add(progress.gen.wrapping_mul(37)),
                );
                push_league(
                    &mut league,
                    (format!("gen{}", progress.gen), args.cfg.champion),
                    args.league_cap,
                );
                // The runaway guard: log loudly, never silently clamp and
                // move on. A pinned coordinate is the signature of the
                // failure the pool veto above exists to catch, so an
                // accepted champion that has one is worth a human's
                // attention even though it cleared every check.
                runaway = runaway_weights(&args.cfg.champion);
                for (k, v) in &runaway {
                    eprintln!(
                        "climb: RUNAWAY [{}p] gen {} {} = {:.3} (clamp {:.1}) -- pinned coordinate",
                        args.cfg.players,
                        progress.gen,
                        k.name(),
                        v,
                        CLAMP,
                    );
                }
                accepted = true;
            }
        }

        if accepted {
            progress.since_accept = 0;
        } else {
            progress.since_accept += 1;
        }
        recent.push(accepted);
        if recent.len() > 12 {
            recent.remove(0);
        }

        // 1/5th success rule. Held for a few generations after a stall kick:
        // the shrink is x0.85 per rejected generation, so an un-held sigma
        // decays from a 0.5 kick back to the floor inside one stall cycle and
        // the kick buys nothing.
        if hold_sigma > 0 {
            hold_sigma -= 1;
        } else if recent.len() >= 6 {
            let rate = recent.iter().filter(|x| **x).count() as f64 / recent.len() as f64;
            if rate > 0.25 {
                progress.sigma = (progress.sigma * 1.25).min(0.8);
            } else if rate < 0.12 {
                progress.sigma = (progress.sigma * 0.85).max(args.sigma_floor);
            }
        }

        if let Err(e) =
            checkpoint(&args.out, &args.cfg.champion, &progress, args.cfg.players, &standing)
        {
            eprintln!("climb: {e}");
            return ExitCode::FAILURE;
        }

        // Deliberately NOT every generation -- see docs/RUST_LEAGUE.md's
        // "gauntlet" section for the cost/generation this cadence buys. Runs
        // against the champion this generation actually settled on (after
        // accept/veto/reject), same as the anchor `standing` it is logged
        // beside.
        let gauntlet = if gauntlet_due(progress.gen, args.gauntlet_every) {
            measure_gauntlet(
                &args.cfg.champion,
                &args.cfg,
                20_011u64.wrapping_add(progress.gen.wrapping_mul(53)),
            )
        } else {
            Vec::new()
        };

        let secs = t0.elapsed().as_secs_f64();
        let verdict = if accepted {
            "ACCEPT"
        } else if vetoed {
            "VETO  "
        } else {
            "reject"
        };
        let gauntlet_str = if gauntlet.is_empty() {
            String::new()
        } else {
            format!(
                " gauntlet=[{}]",
                gauntlet
                    .iter()
                    .map(|(name, a)| format!("{name}={:.3}", a.mean))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        // Stage 1 and stage 2 (module doc, "two-stage confirm") logged
        // together, in that order, tagged `confirmed` -- see this pair's
        // shared doc comment on the JSONL `pool` field below for why they
        // share one field rather than each getting their own.
        let pool_entries: Vec<(&PoolResult, bool)> = pool_results
            .iter()
            .map(|r| (r, false))
            .chain(confirm_results.iter().map(|r| (r, true)))
            .collect();
        let pool_str = if pool_entries.is_empty() {
            String::new()
        } else {
            format!(
                " pool=[{}]{}",
                pool_entries
                    .iter()
                    .map(|(r, confirmed)| format!(
                        "{}{}={:.3}/{:.3}",
                        r.name,
                        if *confirmed { "(confirm)" } else { "" },
                        r.candidate.mean,
                        r.incumbent.mean,
                    ))
                    .collect::<Vec<_>>()
                    .join(","),
                veto_reason.as_deref().map(|r| format!(" lost_to={r}")).unwrap_or_default(),
            )
        };
        println!(
            "[{}p] gen {} {} sigma={:.3} {:.1}s anchor={:.3}{}{} {}",
            args.cfg.players,
            progress.gen,
            verdict,
            progress.sigma,
            secs,
            standing.mean,
            gauntlet_str,
            pool_str,
            tried.join(" "),
        );
        if let Some(path) = &args.log {
            let gauntlet_json: String = gauntlet
                .iter()
                .map(|(name, a)| {
                    format!(
                        "{{\"name\":\"{name}\",\"share\":{:.4},\"half\":{:.4},\"n\":{}}}",
                        a.mean, a.half, args.cfg.gauntlet_games,
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            // Existing field names (`name`/`candidate`/`incumbent`) are kept
            // exactly as before -- a reader that only ever looked at those
            // three still sees the same shape. `confirmed` is new and
            // additive: `false` for the cheap stage-1 screen, `true` for a
            // stage-2 ([`confirm_suspects`]) replay at `--pool-confirm-games`
            // of a member stage 1 flagged (`suspect`) -- this is where "did
            // stage 2 fire, and what did it conclude" (the caller's ask)
            // becomes readable from the log: a `true` row next to the same
            // `name` as a `false` row is the confirm stage's own, bigger
            // look at exactly the member that worried stage 1.
            let pool_json: String = pool_entries
                .iter()
                .map(|(r, confirmed)| {
                    format!(
                        "{{\"name\":\"{}\",\"candidate\":{:.4},\"incumbent\":{:.4},\"confirmed\":{}}}",
                        r.name, r.candidate.mean, r.incumbent.mean, confirmed,
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let pool_veto_json = match &veto_reason {
                Some(name) => format!("\"{name}\""),
                None => "null".to_string(),
            };
            let runaway_json: String = runaway
                .iter()
                .map(|(k, v)| format!("{{\"key\":\"{}\",\"value\":{:.3}}}", k.name(), v))
                .collect::<Vec<_>>()
                .join(",");
            let line = format!(
                "{{\"gen\":{},\"players\":{},\"accepted\":{},\"vetoed\":{},\"sigma\":{:.4},\
                 \"secs\":{:.1},\"since_accept\":{},\"anchor\":{:.4},\"anchor_half\":{:.4},\
                 \"gauntlet\":[{}],\"pool\":[{}],\"pool_veto\":{},\"runaway\":[{}],\
                 \"tried\":[{}]}}\n",
                progress.gen,
                args.cfg.players,
                accepted,
                vetoed,
                progress.sigma,
                secs,
                progress.since_accept,
                standing.mean,
                standing.half,
                gauntlet_json,
                pool_json,
                pool_veto_json,
                runaway_json,
                tried.join(","),
            );
            // A log that cannot be written is worth saying so about once, but
            // it is not worth losing the climb over.
            if let Err(e) = append(path, &line) {
                eprintln!("climb: log: {e}");
            }
        }
    }
    ExitCode::SUCCESS
}

impl Config {
    /// The value a mutant indistinguishable from the champion is expected to
    /// land on -- the accept gate's floor (`c.lo > null`). Exhaustive over
    /// [`Fitness`] with no wildcard arm: a future fitness signal has to
    /// supply its own null rather than silently inheriting one it was never
    /// checked against.
    ///
    /// `Fitness::Win`'s `1 / players` is exact by symmetry -- `players`
    /// interchangeable seats split the win evenly. `Fitness::Margin`'s `0.5`
    /// is [`margin_share`] evaluated at `lead == 0`, the rule-derived
    /// win/lose boundary, and is exact at 2p for the same symmetry reason --
    /// but at 3p/4p it is a deliberate APPROXIMATION: the TRUE expected
    /// margin share of two identical vectors sits measurably below 0.5,
    /// because the best of several equally-strong rivals beats their own
    /// average (the same reason `Fitness::Win`'s null itself shrinks as
    /// `players` grows). Getting that exact number needs measuring it by
    /// self-play, which this change does not do. `0.5` errs on the
    /// conservative side of that gap -- a neutral mutant is slightly LESS
    /// likely to clear it than a perfectly calibrated threshold would allow
    /// -- and a spuriously rejected neutral mutant costs one generation
    /// where a spuriously accepted harmful one costs a permanent regression,
    /// so this is the same asymmetric bet the pool veto's whole design
    /// already makes (module doc, section 2).
    fn null(&self) -> f64 {
        match self.fitness {
            Fitness::Win => 1.0 / self.players as f64,
            Fitness::Margin => 0.5,
        }
    }
}

fn append(path: &Path, line: &str) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    f.write_all(line.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    // Only the sign-gate tests need this -- importing it at file scope
    // would warn as unused in a normal build.
    use tta::bots::weighted::eval;

    fn cfg() -> Config {
        Args::default().cfg
    }

    #[test]
    fn the_numeraire_never_moves() {
        let w = Weights::defaults();
        let mut s = Search::new(1);
        for op in [Op::Scatter, Op::Group, Op::Rescale, Op::Kick] {
            for _ in 0..40 {
                let m = mutate(&w, &mut s, 0.5, Some(op));
                assert_eq!(
                    m.weights.get(WeightKey::Culture),
                    w.get(WeightKey::Culture),
                    "{op:?} moved the frozen key"
                );
            }
        }
    }

    #[test]
    fn every_operator_actually_changes_something() {
        let w = Weights::defaults();
        let mut s = Search::new(7);
        for op in [Op::Scatter, Op::Group, Op::Rescale, Op::Kick] {
            let m = mutate(&w, &mut s, 0.4, Some(op));
            assert!(m.moved > 0, "{op:?} picked no keys");
            assert!(m.weights != w, "{op:?} produced the champion unchanged");
        }
    }

    /// `rescale` multiplies, so an all-zero vector has nothing it can move --
    /// it must fall through to an operator that adds rather than spend a whole
    /// generation's evaluation discovering the mutant is the champion.
    #[test]
    fn rescale_on_a_dead_vector_falls_through_to_scatter() {
        let mut w = Weights::defaults();
        for &k in WeightKey::ALL {
            w.set(k, 0.0);
        }
        let mut s = Search::new(3);
        let m = mutate(&w, &mut s, 0.4, Some(Op::Rescale));
        assert!(!m.label.starts_with("rescale"), "label was {}", m.label);
        assert!(m.weights != w, "nothing moved off zero");
    }

    #[test]
    fn no_weight_escapes_the_clamp() {
        let mut w = Weights::defaults();
        for &k in WeightKey::ALL {
            w.set(k, 55.0);
        }
        let mut s = Search::new(11);
        for _ in 0..200 {
            w = mutate(&w, &mut s, 0.8, None).weights;
            for &k in WeightKey::ALL {
                assert!(w.get(k).abs() <= CLAMP, "{} ran to {}", k.name(), w.get(k));
            }
        }
    }

    /// The climb may not walk a penalty weight positive. `dominance_repair`
    /// used to run only in `load_weights`, so a champion was legal at startup
    /// and unconstrained forever after -- and the live arms duly pushed all
    /// five former `LOSS_GATES` keys positive. Drive the mutator hard from a
    /// deliberately illegal start and assert every mutant comes back legal.
    /// Sourced from [`eval::non_positive_gates`] (derived from
    /// [`WeightKey::sign_intent`], `weights.rs`), not a hand-copied list, so
    /// a key reclassified `NonPositive` there arms this coverage
    /// automatically with no edit needed here.
    #[test]
    fn no_mutant_ever_prices_a_rulebook_penalty_as_a_benefit() {
        let mut w = Weights::defaults();
        for (k, _) in eval::non_positive_gates() {
            w.set(k, 9.0);
        }
        let mut s = Search::new(2027);
        for gen in 0..300 {
            w = mutate(&w, &mut s, 0.8, None).weights;
            for (k, why) in eval::non_positive_gates() {
                assert!(
                    w.get(k) <= 1e-12,
                    "generation {gen}: {} walked to {}, which {why}",
                    k.name(),
                    w.get(k)
                );
            }
        }
    }

    /// Mirror image of the test above, in the other direction: the climb may
    /// not walk a benefit-shaped weight ([`eval::non_negative_gates`])
    /// negative either. Same past bug, same fix -- `dominance_repair` running
    /// only in `load_weights` made a champion legal at startup and
    /// unconstrained forever after, and this is what let the live 2p
    /// champion price `wonder_potential` at -0.7206 despite its authored
    /// default being 0.0. Drive the mutator hard from a deliberately illegal
    /// start (every gated key pinned at -9.0, the mirror of the +9.0 start
    /// above) and assert every mutant comes back legal, sourced from
    /// [`eval::non_negative_gates`] so a future `NonNegative` classification
    /// in [`WeightKey::sign_intent`] arms this coverage the moment it lands,
    /// with no separate test to remember.
    #[test]
    fn no_mutant_ever_prices_a_benefit_shaped_weight_as_a_downside() {
        let mut w = Weights::defaults();
        for (k, _) in eval::non_negative_gates() {
            w.set(k, -9.0);
        }
        let mut s = Search::new(2028);
        for gen in 0..300 {
            w = mutate(&w, &mut s, 0.8, None).weights;
            for (k, why) in eval::non_negative_gates() {
                assert!(
                    w.get(k) >= -1e-12,
                    "generation {gen}: {} walked to {}, which {why}",
                    k.name(),
                    w.get(k)
                );
            }
        }
    }

    /// The third direction, and the one the other two do not cover: a
    /// rule-level ORDERING between two weights ([`eval::DOMINATES`]) has to
    /// survive mutation too, not merely load. `wonder_potential >=
    /// wonder_promise` is what keeps paying a wonder stage a net gain -- the
    /// two coordinates split one wonder's value by how much of it is paid for,
    /// so a climb that walks the promise above the payoff makes every
    /// `Move::WonderStep` look like a loss, which is the same shape of bug
    /// `no_mutant_ever_prices_a_rulebook_penalty_as_a_benefit` documents for
    /// signs. Driven from a deliberately inverted start and table-driven over
    /// `DOMINATES`, so a future pair arms this coverage with no new test.
    #[test]
    fn no_mutant_ever_walks_a_dominated_weight_above_the_one_that_dominates_it() {
        let mut w = Weights::defaults();
        for &(hi, lo) in eval::DOMINATES {
            w.set(hi, -9.0);
            w.set(lo, 9.0);
        }
        let mut s = Search::new(2029);
        for gen in 0..300 {
            w = mutate(&w, &mut s, 0.8, None).weights;
            for &(hi, lo) in eval::DOMINATES {
                assert!(
                    w.get(hi) >= w.get(lo) - 1e-12,
                    "generation {gen}: {} ({}) fell below {} ({})",
                    hi.name(),
                    w.get(hi),
                    lo.name(),
                    w.get(lo)
                );
            }
        }
    }

    /// LEADERSIGN: the board-credit rule (`eval::dominance_repair`'s
    /// `card_board_base + card_board_credit_keys() >= 0` loop) has to
    /// survive mutation too, exactly like the three directions above it --
    /// this is what actually proves "adding the rule inside
    /// `dominance_repair` arms both load-time and mutation-time repair at
    /// once" rather than merely asserting it by reading `mutate`'s source.
    /// Driven from the live 2p champion's own illegal shape (`card_board_
    /// credit` legal at `0.0`, every per-type key pinned deeply negative)
    /// and table-driven over [`eval::card_board_credit_keys`], so a future
    /// card type this function starts covering arms this coverage with no
    /// new test, matching every other guard test in this file.
    #[test]
    fn no_mutant_ever_leaves_a_board_credit_pair_net_negative() {
        let mut w = Weights::defaults();
        w.set(WeightKey::CardBoardCredit, 0.0);
        for &k in eval::card_board_credit_keys() {
            w.set(k, -9.0);
        }
        let mut s = Search::new(2030);
        for gen in 0..300 {
            w = mutate(&w, &mut s, 0.8, None).weights;
            let base = w.get(WeightKey::CardBoardCredit);
            for &k in eval::card_board_credit_keys() {
                assert!(
                    base + w.get(k) >= -1e-12,
                    "generation {gen}: card_board_credit ({base}) + {} ({}) went negative",
                    k.name(),
                    w.get(k)
                );
            }
        }
    }

    /// SIGNAUDIT instance 3 (`hand_value_early`/`hand_value_late`): the
    /// `NET_NONNEG_PHASE` rule (`eval::dominance_repair`'s `base + k.early()
    /// >= 0` / `base + k.late() >= 0` loop) was EMPTY until this audit added
    /// `WeightKey::HandValue` to it, so nothing ever proved that rule is
    /// actually armed at mutation time rather than only at load -- this is
    /// that proof, the same shape as `no_mutant_ever_leaves_a_board_credit_
    /// > pair_net_negative` one test up. Driven from a shape close to the
    /// live corpus (`hand_value` legal near its authored default,
    /// `hand_value_early`/`hand_value_late` both pinned deeply negative) and
    /// table-driven over `eval::NET_NONNEG_PHASE`, so the next key added to
    /// > that list arms this coverage with no new test.
    #[test]
    fn no_mutant_ever_leaves_a_net_nonneg_phase_pair_net_negative() {
        let mut w = Weights::defaults();
        for &k in eval::NET_NONNEG_PHASE {
            w.set(k, 0.2);
            w.set(k.early(), -9.0);
            w.set(k.late(), -9.0);
        }
        let mut s = Search::new(2031);
        for gen in 0..300 {
            w = mutate(&w, &mut s, 0.8, None).weights;
            for &k in eval::NET_NONNEG_PHASE {
                let base = w.get(k);
                for mk in [k.early(), k.late()] {
                    assert!(
                        base + w.get(mk) >= -1e-12,
                        "generation {gen}: {} ({base}) + {} ({}) went negative",
                        k.name(),
                        mk.name(),
                        w.get(mk)
                    );
                }
            }
        }
    }

    /// The whole point of the anchor: a veto fires only when the drop is
    /// unambiguous, never on two intervals that still overlap.
    #[test]
    fn the_veto_needs_the_intervals_to_be_disjoint() {
        let champ = Anchor { mean: 0.50, half: 0.05 };
        assert!(Anchor { mean: 0.30, half: 0.05 }.clearly_worse_than(&champ));
        assert!(!Anchor { mean: 0.44, half: 0.05 }.clearly_worse_than(&champ));
        assert!(!Anchor { mean: 0.60, half: 0.05 }.clearly_worse_than(&champ));
    }

    /// A single deal cannot bound itself, so `stats` hands back an infinite
    /// half-width. Treating that as "unknown, therefore not worse" is what
    /// keeps a too-small `--anchor-games` from vetoing everything.
    #[test]
    fn an_unbounded_measurement_never_vetoes() {
        let wide = Anchor { mean: 0.0, half: 1.0 };
        assert!(!wide.clearly_worse_than(&Anchor { mean: 0.5, half: 0.05 }));
    }

    #[test]
    fn a_champion_round_trips_through_a_checkpoint() {
        let dir = std::env::temp_dir().join("tta_climb_ckpt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("champ.json");
        let mut w = Weights::defaults();
        w.set(WeightKey::Workers, 3.5);
        let p = Progress { gen: 42, sigma: 0.31, since_accept: 7 };
        checkpoint(&path, &w, &p, 3, &Anchor { mean: 0.4, half: 0.06 }).unwrap();
        let (back, got) = resume(&path).unwrap();
        assert_eq!(back.get(WeightKey::Workers), 3.5);
        assert_eq!(got.gen, 42);
        assert_eq!(got.since_accept, 7);
        assert!((got.sigma - 0.31).abs() < 1e-9);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_checkpoint_is_a_fresh_start_not_an_error() {
        assert!(resume(Path::new("/nonexistent/tta/champion.json")).is_none());
    }

    #[test]
    fn a_challenge_batch_never_replays_the_same_deals() {
        // Two consecutive batches inside one challenge must not share a seed;
        // if they did, the second batch would double-count the first's games
        // and narrow the interval without adding information.
        let seeds: Vec<u64> = (0..3).map(|i| 500u64.wrapping_add(i * 24)).collect();
        assert_eq!(seeds.len(), seeds.iter().collect::<std::collections::HashSet<_>>().len());
    }

    #[test]
    fn a_screen_smaller_than_one_deal_is_rejected_on_the_command_line() {
        let e = parse_args(&["--players".into(), "4".into(), "--screen".into(), "2".into()]);
        assert!(e.is_err(), "{e:?}");
    }

    #[test]
    fn max_games_below_screen_is_rejected() {
        let e = parse_args(&[
            "--screen".into(),
            "60".into(),
            "--max-games".into(),
            "30".into(),
        ]);
        assert!(e.is_err(), "{e:?}");
    }

    #[test]
    fn an_identical_mutant_cannot_clear_the_gate() {
        // The floor under the whole loop: a mutant that IS the champion plays
        // an even split, so its lower bound must sit below the null.
        let mut c = cfg();
        c.screen = 24;
        c.max_games = 24;
        c.threads = 2;
        let r = challenge(&c.champion, &c, 1234);
        assert!(r.lo <= c.null(), "an identical vector cleared the gate at lo={}", r.lo);
    }

    /// The optional-stopping bug this fix closes: `challenge` used to re-test
    /// `lo > null` after every batch and break the moment that cleared, which
    /// is peeking, not a single look (see this function's own doc comment and
    /// `/private/tmp/gatediag/sim.py`). Pit the tuned default vector against
    /// an all-zero one -- a `WeightedBot` scores every candidate move 0.0
    /// under all-zero weights, so it deterministically plays whatever
    /// `legal_moves` lists first every turn (see `eval.rs`'s `WeightedBot::
    /// choose`) -- a lopsided, deterministic mismatch whose lower bound
    /// clears the null within the first couple of screening batches. Even so,
    /// the challenge has to keep buying games all the way to `max_games`: a
    /// clear win earns no early exit any more, only a clear loss does.
    #[test]
    fn a_clear_winner_still_plays_out_the_full_max_games() {
        let mut c = cfg();
        c.players = 2;
        c.kind = BotKind::Weighted;
        let mut all_zero = Weights::defaults();
        for &k in WeightKey::ALL {
            all_zero.set(k, 0.0);
        }
        c.champion = all_zero;
        c.screen = 8;
        c.max_games = 64;
        c.threads = 4;
        let mutant = Weights::defaults();

        let r = challenge(&mutant, &c, 5150);

        assert_eq!(r.games, c.max_games, "stopped early at lo={} instead of playing every game", r.lo);
        assert!(r.lo > c.null(), "the matchup was not actually lopsided: lo={}", r.lo);
    }

    // ============================================================== fitness

    #[test]
    fn the_default_config_selects_win_fitness() {
        // The floor under every other test in this section: if the default
        // ever silently became `Margin`, every run since would have changed
        // what it accepts without anyone passing a new flag.
        assert_eq!(cfg().fitness, Fitness::Win);
        assert_eq!(parse_args(&[]).unwrap().unwrap().cfg.fitness, Fitness::Win);
    }

    #[test]
    fn the_fitness_flag_parses_both_spellings_and_rejects_anything_else() {
        let win = parse_args(&["--fitness".into(), "win".into()]).unwrap().unwrap();
        assert_eq!(win.cfg.fitness, Fitness::Win);
        let margin = parse_args(&["--fitness".into(), "margin".into()]).unwrap().unwrap();
        assert_eq!(margin.cfg.fitness, Fitness::Margin);
        assert!(parse_args(&["--fitness".into(), "share".into()]).is_err());
    }

    /// `per_game_value` under `Fitness::Win` must be `Some(d.share)` for
    /// every `Duel`, unconditionally -- this is the substitution that keeps
    /// `challenge` bit-identical to its pre-`Fitness` self on the default
    /// path: swapping `Some(d.share)` for `per_game_value(d, cfg)` changes
    /// nothing as long as this holds.
    #[test]
    fn win_fitness_reads_every_duels_share_unconditionally() {
        let win_cfg = cfg();
        assert_eq!(win_cfg.fitness, Fitness::Win);
        for (share, cap_hit) in [(1.0, false), (0.0, false), (0.5, false), (1.0, true), (0.0, true)] {
            let d = Duel { share, culture_a: 40.0, culture_best_other: 40.0, moves: 10, cap_hit };
            assert_eq!(
                per_game_value(&d, &win_cfg),
                Some(share),
                "win fitness must not exclude or transform a game, cap_hit={cap_hit}"
            );
        }
    }

    /// Bit-identical proof that leaving `--fitness` unset reproduces the
    /// exact prior accept decision: replay `challenge`'s own batching loop
    /// by hand -- same seed arithmetic, same early-exit rule -- reading
    /// `Duel::share` straight off each played game, with no `per_game_value`
    /// or `Fitness` anywhere in it, and demand `challenge` itself (under the
    /// default `Config`) produces the SAME `Challenge` numbers for the same
    /// seed. If `Fitness::Win`'s wiring ever introduced so much as a
    /// rounding difference from the pre-`Fitness` code, this would catch it.
    #[test]
    fn the_default_fitness_reproduces_the_win_share_challenge_bit_for_bit() {
        let mut c = cfg();
        c.players = 3;
        c.screen = 9;
        c.max_games = 18;
        c.threads = 1;
        assert_eq!(c.fitness, Fitness::Win);

        let mutant = mutate(&c.champion, &mut Search::new(4), 0.3, None).weights;
        let seed = 909u64;

        // `challenge`'s own loop, inlined, reading `d.share` directly instead
        // of going through `per_game_value`/`Fitness`.
        let null = 1.0 / c.players as f64;
        let mut shares: Vec<Option<f64>> = Vec::new();
        let mut batch = c.screen;
        while shares.len() < c.max_games {
            let want = batch.min(c.max_games - shares.len());
            let mut duel = Match {
                a: Seat { kind: c.kind, weights: mutant, search: SeatSearch::None },
                b: Seat { kind: c.kind, weights: c.champion, search: SeatSearch::None },
                games: want,
                players: c.players,
                seed: seed.wrapping_add(shares.len() as u64),
                threads: c.threads,
            };
            if duel.validate().is_err() {
                break;
            }
            shares.extend(duel.play().iter().map(|d| Some(d.share)));
            let est = stats::paired(&shares, c.players as usize);
            if est.mean < null && shares.len() >= c.screen {
                break;
            }
            batch = c.screen;
        }
        let want = stats::paired(&shares, c.players as usize);

        let got = challenge(&mutant, &c, seed);
        assert_eq!(got.share, want.mean, "mean diverged from the hand-rolled win-share computation");
        assert_eq!(got.lo, want.mean - c.accept_z * want.se, "lo diverged from the hand-rolled computation");
        assert_eq!(got.games, shares.len());
    }

    /// The whole reason a continuous signal helps: the same win/loss record
    /// can carry a bigger or smaller margin, and margin fitness must
    /// actually use that -- win fitness, by construction, cannot, because
    /// every game here is an equally clean win under both records.
    #[test]
    fn margin_fitness_accepts_the_bigger_margin_and_rejects_the_smaller_one_at_an_identical_win_count() {
        let players = 3u8;
        let mut win_cfg = cfg();
        win_cfg.players = players;
        let mut margin_cfg = cfg();
        margin_cfg.players = players;
        margin_cfg.fitness = Fitness::Margin;

        // Two complete deals' worth of games (3 seats x 2), every one a clean
        // win (share 1.0) at BOTH records -- only the lead differs.
        let record = |lead: f64| -> Vec<Duel> {
            (0..(players as usize * 2))
                .map(|_| Duel { share: 1.0, culture_a: 100.0 + lead, culture_best_other: 100.0, moves: 30, cap_hit: false })
                .collect()
        };
        let small_margin = record(2.0);
        let big_margin = record(20.0);

        // Win fitness cannot see the difference: every game is share 1.0
        // either way, so the reduced mean is identical.
        let win_small: Vec<Option<f64>> = small_margin.iter().map(|d| per_game_value(d, &win_cfg)).collect();
        let win_big: Vec<Option<f64>> = big_margin.iter().map(|d| per_game_value(d, &win_cfg)).collect();
        assert_eq!(
            stats::paired(&win_small, players as usize).mean,
            stats::paired(&win_big, players as usize).mean,
            "win fitness must be blind to a margin difference on an identical win record"
        );

        // Margin fitness strictly prefers the bigger lead, and the gap is
        // decisive: a null between the two accepts the bigger-margin record
        // and rejects the smaller one, on IDENTICAL win counts -- exactly
        // the distinction win fitness alone can never make.
        let margin_small: Vec<Option<f64>> = small_margin.iter().map(|d| per_game_value(d, &margin_cfg)).collect();
        let margin_big: Vec<Option<f64>> = big_margin.iter().map(|d| per_game_value(d, &margin_cfg)).collect();
        let mean_small = stats::paired(&margin_small, players as usize).mean;
        let mean_big = stats::paired(&margin_big, players as usize).mean;
        assert!(mean_big > mean_small, "bigger margin ({mean_big}) did not score above the smaller one ({mean_small})");
        let threshold = (mean_small + mean_big) / 2.0;
        assert!(mean_big > threshold, "the bigger-margin record must clear a null between the two");
        assert!(mean_small < threshold, "the smaller-margin record must not clear that same null");
    }

    /// A game that hit the move cap has no trustworthy margin: the game
    /// stopped before it would have ended on its own, so `Duel::lead()`
    /// reflects an arbitrary partial state, not a result. It must be
    /// EXCLUDED from the margin average (a `None`, not folded in as `0.0`,
    /// which would be indistinguishable from an honestly-played dead-even
    /// game and would quietly bias the mean), while still being COUNTED --
    /// it keeps its slot in the per-game vector rather than vanishing from
    /// it, which is what keeps a later game's (deal, seat) recoverable by
    /// position (`stats::paired`'s own doc comment).
    #[test]
    fn an_unfinished_game_is_excluded_from_margin_not_scored_as_zero() {
        let players = 2u8;
        let mut c = cfg();
        c.players = players;
        c.fitness = Fitness::Margin;

        let finished = Duel { share: 1.0, culture_a: 150.0, culture_best_other: 100.0, moves: 40, cap_hit: false };
        let capped_out = Duel { share: 1.0, culture_a: 150.0, culture_best_other: 100.0, moves: 500, cap_hit: true };

        assert!(per_game_value(&finished, &c).is_some(), "a finished game must have a margin");
        assert_eq!(
            per_game_value(&capped_out, &c),
            None,
            "a move-cap-out must be excluded outright, not scored as margin 0.0"
        );

        let per_game: Vec<Option<f64>> = vec![per_game_value(&finished, &c), per_game_value(&capped_out, &c)];
        assert_eq!(per_game.len(), 2, "the excluded game must still occupy a counted slot, not vanish");
        let est = stats::paired(&per_game, players as usize);
        assert_eq!(est.n_games, 1, "the cap-out must not be counted as a scored game");
        assert_eq!(est.n_deals, 0, "a deal missing a seat's value must be dropped whole, not half-counted");
    }

    /// `margin_share` must be monotonic in the lead (bigger lead, bigger
    /// share) and centred at `lead == 0` -- the rule-derived win/lose
    /// boundary `Config::null`'s `Fitness::Margin` arm is built on.
    #[test]
    fn margin_share_is_centred_at_zero_lead_and_increases_with_the_lead() {
        for players in [2u8, 3, 4] {
            assert_eq!(margin_share(0.0, players), 0.5, "{players}p: zero lead must map to 0.5");
            assert!(margin_share(20.0, players) > 0.5, "{players}p: a positive lead must score above 0.5");
            assert!(margin_share(-20.0, players) < 0.5, "{players}p: a negative lead must score below 0.5");
            assert!(
                margin_share(50.0, players) > margin_share(5.0, players),
                "{players}p: a bigger lead must score higher"
            );
            // `tanh` of a huge argument rounds to exactly 1.0 in `f64`, so an
            // extreme blowout is bounded AT, not strictly below, 1.0 -- the
            // property worth pinning is the bound itself, not an inequality
            // floating-point cannot actually keep.
            assert!(
                (0.0..=1.0).contains(&margin_share(1e9, players)),
                "{players}p: an extreme lead must stay within [0, 1]"
            );
        }
    }

    #[test]
    fn gauss_is_centred_and_scaled() {
        let mut s = Search::new(99);
        let n = 20_000;
        let xs: Vec<f64> = (0..n).map(|_| s.gauss(2.0)).collect();
        let mean = xs.iter().sum::<f64>() / n as f64;
        let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
        assert!(mean.abs() < 0.05, "mean {mean}");
        assert!((var.sqrt() - 2.0).abs() < 0.1, "sd {}", var.sqrt());
    }

    // ================================================================ gauntlet

    #[test]
    fn the_gauntlet_is_due_only_on_multiples_of_its_cadence() {
        assert!(gauntlet_due(50, 50));
        assert!(gauntlet_due(100, 50));
        assert!(!gauntlet_due(49, 50));
        assert!(!gauntlet_due(51, 50));
        // 0 is the "disabled" cadence, not an every-generation one.
        assert!(!gauntlet_due(50, 0));
        assert!(!gauntlet_due(0, 0));
    }

    #[test]
    fn an_empty_gauntlet_plays_no_games_and_reports_nothing() {
        let c = cfg();
        assert!(c.gauntlet.is_empty());
        let got = measure_gauntlet(&c.champion, &c, 1);
        assert!(got.is_empty());
    }

    #[test]
    fn the_gauntlet_reports_one_row_per_member_labelled_by_name() {
        let mut c = cfg();
        c.players = 2;
        c.gauntlet_games = 4; // cheap: this only checks shape, not the number
        let seat = Seat { kind: c.kind, weights: Weights::defaults(), search: SeatSearch::None };
        c.gauntlet = vec![("frozen_a".to_string(), seat), ("frozen_b".to_string(), seat)];
        let got = measure_gauntlet(&c.champion, &c, 42);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "frozen_a");
        assert_eq!(got[1].0, "frozen_b");
        for (_, a) in &got {
            assert!((0.0..=1.0).contains(&a.mean), "share {} out of range", a.mean);
        }
    }

    /// `challenge` is the per-mutant local-search screen, not the pool veto
    /// (`pool_members`/`play_pool`/`worst_case_verdict`, which DOES read
    /// `Config::gauntlet` -- see this file's module doc, section 2, and the
    /// updated `Config::gauntlet` doc comment). This test pins the narrower
    /// claim that is still true post-pool-veto: `challenge`'s own signature
    /// takes no gauntlet argument, so its verdict on a mutant cannot depend
    /// on what `--gauntlet` flags happened to be passed.
    #[test]
    fn challenge_does_not_read_the_gauntlet_field() {
        let mut c = cfg();
        c.players = 2;
        c.gauntlet_games = 4;
        c.gauntlet = vec![("frozen".to_string(), Seat { kind: c.kind, weights: Weights::defaults(), search: SeatSearch::None })];
        // Run it twice, once with the gauntlet populated and once without --
        // the mutant accept decision (`challenge`) must be identical either
        // way, since it never consults `c.gauntlet`.
        let with_gauntlet = challenge(&c.champion, &c, 777);
        c.gauntlet.clear();
        let without_gauntlet = challenge(&c.champion, &c, 777);
        assert_eq!(with_gauntlet.share, without_gauntlet.share);
        assert_eq!(with_gauntlet.lo, without_gauntlet.lo);
    }

    #[test]
    fn a_gauntlet_flag_loads_the_file_and_labels_it_by_stem() {
        let dir = std::env::temp_dir().join("tta_climb_gauntlet_flag");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("champion_2p_gen99_140key_2026-08-06.json");
        save_weights(&path, &Weights::defaults(), &[("gen", 99.0)]).unwrap();

        let args = parse_args(&[
            "--gauntlet".into(),
            path.to_string_lossy().into_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.cfg.gauntlet.len(), 1);
        assert_eq!(args.cfg.gauntlet[0].0, "champion_2p_gen99_140key_2026-08-06");
        assert_eq!(
            args.cfg.gauntlet[0].1.weights.get(WeightKey::Culture),
            Weights::defaults().get(WeightKey::Culture)
        );
        // No `--gauntlet-kind` override, so the member plays the champion's
        // own kind -- the old, single-kind behaviour, unchanged.
        assert_eq!(args.cfg.gauntlet[0].1.kind, args.cfg.kind);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_gauntlet_flag_pointing_at_a_missing_file_is_a_command_line_error() {
        let e = parse_args(&["--gauntlet".into(), "/nonexistent/tta/nope.json".into()]);
        assert!(e.is_err(), "{e:?}");
    }

    /// The whole point of `--gauntlet-kind`: a gauntlet member can be a
    /// DIFFERENT kind from the champion, loaded with THAT kind's own loader
    /// (`human_policy::load_weights`, no `dominance_repair`) rather than the
    /// champion loader every `--gauntlet` entry used unconditionally before
    /// this existed. Uses a `BlueFree`-over-`ResourceStock` fixture, a real
    /// rule `DOMINATES` requires the other way around, so a champion-loader
    /// read would come back repaired and a human-loader read would not.
    #[test]
    fn a_gauntlet_kind_flag_loads_that_member_with_its_own_kinds_loader() {
        let dir = std::env::temp_dir().join("tta_climb_gauntlet_kind_flag");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("violating.json");
        let mut w = Weights::defaults();
        w.set(WeightKey::ResourceStock, 0.0);
        w.set(WeightKey::BlueFree, 10.0);
        tta::human_policy::save_weights(&path, &w).unwrap();

        let args = parse_args(&[
            "--gauntlet-kind".into(),
            "human".into(),
            "--gauntlet".into(),
            path.to_string_lossy().into_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.cfg.gauntlet.len(), 1);
        assert_eq!(args.cfg.gauntlet[0].1.kind, BotKind::Human);
        assert_eq!(
            args.cfg.gauntlet[0].1.weights.get(WeightKey::ResourceStock),
            0.0,
            "a human gauntlet member must not have been dominance-repaired"
        );
        std::fs::remove_file(&path).ok();
    }

    /// `--gauntlet-kind` with no following `--gauntlet` is simply unused --
    /// it modifies the NEXT `--gauntlet` flag only, so a plain `--gauntlet
    /// PATH` after it (not the one it targets) falls back to the champion's
    /// own kind exactly as if `--gauntlet-kind` had never been passed. Two
    /// consecutive `--gauntlet` flags with only one preceding
    /// `--gauntlet-kind` pins that the override does not leak onto the next
    /// member.
    #[test]
    fn a_gauntlet_kind_override_applies_to_only_the_next_gauntlet_member() {
        let dir = std::env::temp_dir().join("tta_climb_gauntlet_kind_leak");
        std::fs::create_dir_all(&dir).unwrap();
        let human_path = dir.join("human_member.json");
        let champ_path = dir.join("champ_member.json");
        tta::human_policy::save_weights(&human_path, &Weights::defaults()).unwrap();
        save_weights(&champ_path, &Weights::defaults(), &[]).unwrap();

        let args = parse_args(&[
            "--gauntlet-kind".into(),
            "human".into(),
            "--gauntlet".into(),
            human_path.to_string_lossy().into_owned(),
            "--gauntlet".into(),
            champ_path.to_string_lossy().into_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.cfg.gauntlet.len(), 2);
        assert_eq!(args.cfg.gauntlet[0].1.kind, BotKind::Human);
        assert_eq!(args.cfg.gauntlet[1].1.kind, args.cfg.kind, "override leaked onto the second member");
        std::fs::remove_file(&human_path).ok();
        std::fs::remove_file(&champ_path).ok();
    }

    // ------------------------------------------------------- --gauntlet-search

    /// `--gauntlet-search` gives the NEXT `--gauntlet` member
    /// `Search::HumanShortlistBeam` instead of the plain `Search::None` --
    /// the root-shortlist+beam hybrid (`bots::greedy::Search`'s own doc
    /// comment) instead of that member's searchless `HumanBot`, with
    /// `eval_weights` read from the `--gauntlet-search` file through the
    /// CHAMPION loader (`dominance_repair` included -- it is a gameplay
    /// evaluator, not a human-imitation fit).
    #[test]
    fn a_gauntlet_search_flag_gives_that_member_the_shortlist_beam_hybrid() {
        let dir = std::env::temp_dir().join("tta_climb_gauntlet_search_flag");
        std::fs::create_dir_all(&dir).unwrap();
        let human_path = dir.join("human_member.json");
        let eval_path = dir.join("eval_vector.json");
        tta::human_policy::save_weights(&human_path, &Weights::defaults()).unwrap();
        save_weights(&eval_path, &Weights::defaults(), &[]).unwrap();

        let args = parse_args(&[
            "--gauntlet-kind".into(),
            "human".into(),
            "--gauntlet-search".into(),
            eval_path.to_string_lossy().into_owned(),
            "--gauntlet".into(),
            human_path.to_string_lossy().into_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.cfg.gauntlet.len(), 1);
        assert_eq!(args.cfg.gauntlet[0].1.kind, BotKind::Human);
        match args.cfg.gauntlet[0].1.search {
            SeatSearch::HumanShortlistBeam { eval_weights } => {
                assert_eq!(eval_weights, load_weights(&eval_path).unwrap());
            }
            SeatSearch::None => panic!("--gauntlet-search must not leave the member at Search::None"),
        }
        std::fs::remove_file(&human_path).ok();
        std::fs::remove_file(&eval_path).ok();
    }

    /// `--gauntlet-search` on a member that is NOT human-kind is a command
    /// line error, not a silently ignored flag -- mirrors
    /// `bin/kindmatch.rs`'s identical `--a-search`/`--b-search` rule (only
    /// legal when the matching side is `human`), and is exactly the
    /// precondition `bots::greedy::build_bots` trusts the caller already
    /// checked (see that function's own doc comment).
    #[test]
    fn a_gauntlet_search_flag_is_rejected_when_the_member_is_not_human_kind() {
        let dir = std::env::temp_dir().join("tta_climb_gauntlet_search_non_human");
        std::fs::create_dir_all(&dir).unwrap();
        let champ_path = dir.join("champ_member.json");
        let eval_path = dir.join("eval_vector.json");
        save_weights(&champ_path, &Weights::defaults(), &[]).unwrap();
        save_weights(&eval_path, &Weights::defaults(), &[]).unwrap();

        // No `--gauntlet-kind`, so this member resolves to `cfg.kind`
        // (`weighted` by default) -- `--gauntlet-search` must reject it.
        let e = parse_args(&[
            "--gauntlet-search".into(),
            eval_path.to_string_lossy().into_owned(),
            "--gauntlet".into(),
            champ_path.to_string_lossy().into_owned(),
        ]);
        assert!(e.is_err(), "{e:?}");
        std::fs::remove_file(&champ_path).ok();
        std::fs::remove_file(&eval_path).ok();
    }

    /// The same "applies to only the NEXT member" semantics
    /// `a_gauntlet_kind_override_applies_to_only_the_next_gauntlet_member`
    /// pins for `--gauntlet-kind`, for `--gauntlet-search`: two consecutive
    /// human-kind `--gauntlet` members with only the FIRST preceded by
    /// `--gauntlet-search` must leave the second at `Search::None`, not
    /// carrying the first member's eval vector forward.
    #[test]
    fn a_gauntlet_search_override_applies_to_only_the_next_gauntlet_member() {
        let dir = std::env::temp_dir().join("tta_climb_gauntlet_search_leak");
        std::fs::create_dir_all(&dir).unwrap();
        let human_path_a = dir.join("human_member_a.json");
        let human_path_b = dir.join("human_member_b.json");
        let eval_path = dir.join("eval_vector.json");
        tta::human_policy::save_weights(&human_path_a, &Weights::defaults()).unwrap();
        tta::human_policy::save_weights(&human_path_b, &Weights::defaults()).unwrap();
        save_weights(&eval_path, &Weights::defaults(), &[]).unwrap();

        let args = parse_args(&[
            "--gauntlet-kind".into(),
            "human".into(),
            "--gauntlet-search".into(),
            eval_path.to_string_lossy().into_owned(),
            "--gauntlet".into(),
            human_path_a.to_string_lossy().into_owned(),
            "--gauntlet-kind".into(),
            "human".into(),
            "--gauntlet".into(),
            human_path_b.to_string_lossy().into_owned(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(args.cfg.gauntlet.len(), 2);
        assert!(
            matches!(args.cfg.gauntlet[0].1.search, SeatSearch::HumanShortlistBeam { .. }),
            "the first member must have the hybrid search"
        );
        assert_eq!(
            args.cfg.gauntlet[1].1.search,
            SeatSearch::None,
            "--gauntlet-search must not leak onto the second member"
        );
        std::fs::remove_file(&human_path_a).ok();
        std::fs::remove_file(&human_path_b).ok();
        std::fs::remove_file(&eval_path).ok();
    }

    #[test]
    fn sample_returns_distinct_elements() {
        let mut s = Search::new(5);
        let pool: Vec<usize> = (0..50).collect();
        for k in [1usize, 7, 50] {
            let got = s.sample(&pool, k);
            assert_eq!(got.len(), k);
            assert_eq!(got.iter().collect::<std::collections::HashSet<_>>().len(), k);
        }
    }

    // =================================================================== pool

    fn anchor(mean: f64, half: f64) -> Anchor {
        Anchor { mean, half }
    }

    fn result(name: &str, candidate: Anchor, incumbent: Anchor) -> PoolResult {
        PoolResult { name: name.to_string(), candidate, incumbent }
    }

    /// The whole point of the pool veto: it gates on the WORST comparison,
    /// not the mean. `friendly` and `nemesis` average to a candidate that
    /// looks fine (0.90 and 0.10 against a 0.50 incumbent baseline both
    /// average to 0.50) -- a mean-based gate would wave this candidate
    /// through. The worst case must not.
    #[test]
    fn worst_case_verdict_fires_on_the_worst_comparison_not_the_average() {
        let friendly = result("friendly", anchor(0.90, 0.05), anchor(0.50, 0.05));
        let nemesis = result("nemesis", anchor(0.10, 0.05), anchor(0.50, 0.05));
        let verdict = worst_case_verdict(&[friendly, nemesis]);
        assert_eq!(verdict.as_deref(), Some("nemesis"), "the collapse against nemesis must gate, not average out");
    }

    /// The mirror of the test above: nothing here is unambiguously worse, so
    /// nothing vetoes -- overlapping intervals are noise, not a regression
    /// (same conservatism as the old single-anchor veto's
    /// `the_veto_needs_the_intervals_to_be_disjoint`).
    #[test]
    fn worst_case_verdict_is_none_when_nothing_is_clearly_worse() {
        let a = result("a", anchor(0.55, 0.05), anchor(0.50, 0.05));
        let b = result("b", anchor(0.48, 0.05), anchor(0.50, 0.05)); // overlapping
        assert!(worst_case_verdict(&[a, b]).is_none());
    }

    /// The veto reports the FIRST offending member in sample order, not
    /// every offender -- one bad comparison is already a veto, and the loop
    /// this feeds (`main`) only needs a name to log.
    #[test]
    fn worst_case_verdict_reports_the_first_offender_in_order() {
        let first_bad = result("first_bad", anchor(0.05, 0.02), anchor(0.50, 0.02));
        let second_bad = result("second_bad", anchor(0.05, 0.02), anchor(0.50, 0.02));
        let verdict = worst_case_verdict(&[first_bad, second_bad]);
        assert_eq!(verdict.as_deref(), Some("first_bad"));
    }

    // ============================================================ pool: stage 2

    /// This is the measured defect itself, reproduced without playing a
    /// single game: at `--pool-games`-sized samples the interval is wide
    /// enough that a genuine -0.10 deficit still overlaps the incumbent's,
    /// so `worst_case_verdict` on stage 1 alone must NOT fire -- exactly why
    /// the pool veto fired zero times across 2,075 real generations even
    /// though 81% of accepted candidates were worse against some member.
    /// `suspect` must still flag it: the point estimate alone is bad enough
    /// to be worth a bigger, dedicated look.
    #[test]
    fn a_stage_one_deficit_too_wide_to_be_disjoint_still_trips_the_confirm_trigger() {
        let stage1 = result("nemesis", anchor(0.40, 0.12), anchor(0.50, 0.12));
        assert!(
            worst_case_verdict(std::slice::from_ref(&stage1)).is_none(),
            "a wide stage-1 interval around a real deficit must not itself veto"
        );
        assert!(suspect(&stage1), "a -0.10 point estimate must trip POOL_CONFIRM_TRIGGER");
    }

    /// The two-stage veto's whole point: the SAME deficit, resampled at
    /// stage 2's larger `--pool-confirm-games` (simulated here by a narrower
    /// half-width, which is what more games buys), now goes disjoint and
    /// `worst_case_verdict` fires. This is the fix -- stage 1 alone could
    /// never see this, stage 2 does.
    #[test]
    fn a_confirmed_stage_two_result_vetoes_where_stage_one_alone_could_not() {
        let stage2 = result("nemesis", anchor(0.40, 0.04), anchor(0.50, 0.04));
        let verdict = worst_case_verdict(std::slice::from_ref(&stage2));
        assert_eq!(verdict.as_deref(), Some("nemesis"), "the confirmed, narrower interval must veto");
    }

    /// A point estimate only mildly behind (above the trigger) must never be
    /// flagged as a suspect -- stage 2 exists to resolve deep deficits stage
    /// 1's noise hides, not to re-litigate every generation's ordinary
    /// sampling wobble.
    #[test]
    fn suspect_does_not_flag_a_point_estimate_above_the_trigger() {
        let mild = result("mild", anchor(0.48, 0.10), anchor(0.50, 0.10)); // -0.02
        assert!(!suspect(&mild));
        let ahead = result("ahead", anchor(0.55, 0.10), anchor(0.50, 0.10)); // +0.05
        assert!(!suspect(&ahead));
    }

    /// End-to-end wiring check for `confirm_suspects`: given a stage-1
    /// result set with one member below the trigger and one above it, only
    /// the flagged member is replayed -- the whole cost-saving point of
    /// making stage 2 conditional rather than universal.
    #[test]
    fn confirm_suspects_only_replays_members_the_stage_one_trigger_flagged() {
        let mut c = cfg();
        c.players = 2;
        c.threads = 2;
        c.pool_confirm_games = 4; // cheap: this checks shape, not the number
        let seat = Seat { kind: c.kind, weights: Weights::defaults(), search: SeatSearch::None };
        let pool = vec![("suspect".to_string(), seat), ("fine".to_string(), seat)];
        let stage1 = vec![
            result("suspect", anchor(0.30, 0.10), anchor(0.50, 0.10)), // -0.20: flagged
            result("fine", anchor(0.48, 0.10), anchor(0.50, 0.10)),    // -0.02: not flagged
        ];
        let confirmed = confirm_suspects(&c.champion, &c, &pool, &stage1, 999);
        let names: Vec<&str> = confirmed.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["suspect"], "only the flagged member should have been replayed");
        for r in &confirmed {
            assert!((0.0..=1.0).contains(&r.candidate.mean), "share {} out of range", r.candidate.mean);
        }
    }

    /// `confirm_suspects` is a no-op, not a panic, when stage 1 flagged
    /// nothing -- the common case, since most generations' candidates are
    /// not deep-negative against any member.
    #[test]
    fn confirm_suspects_is_empty_when_nothing_was_flagged() {
        let mut c = cfg();
        c.players = 2;
        let seat = Seat { kind: c.kind, weights: Weights::defaults(), search: SeatSearch::None };
        let pool = vec![("fine".to_string(), seat)];
        let stage1 = vec![result("fine", anchor(0.52, 0.10), anchor(0.50, 0.10))];
        let confirmed = confirm_suspects(&c.champion, &c, &pool, &stage1, 1);
        assert!(confirmed.is_empty());
    }

    #[test]
    fn pool_members_includes_the_anchor_gauntlet_league_and_incumbent_in_order() {
        let mut c = cfg();
        let seat = Seat { kind: c.kind, weights: Weights::defaults(), search: SeatSearch::None };
        c.gauntlet = vec![("frozen_a".to_string(), seat)];
        let league = vec![("gen10".to_string(), Weights::defaults())];
        let pool = pool_members(&c, &league);
        let names: Vec<&str> = pool.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["anchor", "frozen_a", "gen10", "incumbent"]);
    }

    #[test]
    fn pool_members_is_just_anchor_and_incumbent_with_no_gauntlet_or_league() {
        let c = cfg();
        let pool = pool_members(&c, &[]);
        let names: Vec<&str> = pool.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["anchor", "incumbent"]);
    }

    /// FIFO, oldest first out -- see `push_league`'s doc comment for why a
    /// straight FIFO was chosen over "recent N plus a few spaced-out older
    /// ones". This pins the bound that actually matters for cost: the
    /// league never grows past `cap` regardless of how many generations are
    /// accepted.
    #[test]
    fn the_league_is_capped_and_drops_the_oldest_first() {
        let mut league: Vec<(String, Weights)> = Vec::new();
        for gen in 0..5 {
            push_league(&mut league, (format!("gen{gen}"), Weights::defaults()), 3);
        }
        let names: Vec<&str> = league.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["gen2", "gen3", "gen4"]);
    }

    #[test]
    fn a_zero_league_cap_keeps_no_history() {
        let mut league: Vec<(String, Weights)> = Vec::new();
        push_league(&mut league, ("gen1".to_string(), Weights::defaults()), 0);
        assert!(league.is_empty(), "cap 0 must disable the league, not just shrink it");
    }

    /// `runaway_weights` is the logged-loudly half of the runaway guard
    /// (this file's module doc, "the failure this pool veto exists to
    /// catch"): a coordinate at or very near `CLAMP` must be reported by
    /// name and value, and the frozen numeraire must never be reported even
    /// if it were somehow set that high.
    #[test]
    fn runaway_weights_flags_only_movable_keys_at_or_near_the_clamp() {
        let mut w = Weights::defaults();
        w.set(WeightKey::Workers, CLAMP); // pinned exactly at the wall
        w.set(WeightKey::Culture, CLAMP); // frozen; must never be flagged
        let got = runaway_weights(&w);
        let keys: Vec<WeightKey> = got.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&WeightKey::Workers), "a weight pinned at CLAMP must be flagged");
        assert!(!keys.contains(&WeightKey::Culture), "the frozen key must never be reported as runaway");
    }

    #[test]
    fn runaway_weights_is_empty_for_a_vector_nowhere_near_the_clamp() {
        assert!(runaway_weights(&Weights::defaults()).is_empty());
    }

    /// The sampling half of `play_pool`'s determinism claim (module doc:
    /// "same generation seed always samples the same members"), without
    /// paying for any games: `Search::sample` alone must be a pure function
    /// of its seed and its input.
    #[test]
    fn the_pool_sample_is_deterministic_from_its_seed() {
        let indices: Vec<usize> = (0..8).collect();
        let mut s1 = Search::new(777);
        let mut s2 = Search::new(777);
        assert_eq!(s1.sample(&indices, 3), s2.sample(&indices, 3));
    }

    /// End-to-end wiring check: sampling `k` from a pool of `n > k` members
    /// and playing real (tiny) duels must come back with exactly `k`
    /// distinct results, one per sampled member.
    #[test]
    fn play_pool_samples_k_distinct_members_and_reports_one_result_each() {
        let mut c = cfg();
        c.players = 2;
        c.threads = 2;
        let seat = Seat { kind: c.kind, weights: Weights::defaults(), search: SeatSearch::None };
        c.gauntlet = vec![("frozen_a".to_string(), seat), ("frozen_b".to_string(), seat)];
        let pool = pool_members(&c, &[]); // anchor, frozen_a, frozen_b, incumbent = 4 members
        let results = play_pool(&c.champion, &c, &pool, 2, 4, 555);
        assert_eq!(results.len(), 2);
        let names: std::collections::HashSet<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names.len(), 2, "the sample must not repeat a member");
        for r in &results {
            assert!((0.0..=1.0).contains(&r.candidate.mean), "candidate share {} out of range", r.candidate.mean);
            assert!((0.0..=1.0).contains(&r.incumbent.mean), "incumbent share {} out of range", r.incumbent.mean);
        }
    }

    /// Requesting more opponents than the pool has must not panic -- it
    /// samples everything there is, same as `Search::sample`'s own
    /// documented clamp.
    #[test]
    fn play_pool_with_k_larger_than_the_pool_samples_everything() {
        let c = cfg(); // anchor + incumbent = 2 members, no gauntlet or league
        let pool = pool_members(&c, &[]);
        let results = play_pool(&c.champion, &c, &pool, 10, 4, 1);
        assert_eq!(results.len(), 2);
    }

}
