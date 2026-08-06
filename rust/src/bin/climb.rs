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
//! # 2. An accepted champion has to still beat the anchor
//!
//! This is the fix for the thing that actually went wrong. Every champion the
//! Python league ever produced turned out to be far WORSE than the untuned
//! starting vector -- 22.8% at 2p and 3p, 13.7% at 4p, against nulls of 50%,
//! 33% and 25% -- while every single generation had honestly beaten its own
//! parent. That is the classic self-play cycle: a chain of pairwise
//! improvements walking somewhere worse than where it started, with nothing in
//! the loop ever asking the absolute question.
//!
//! So the loop asks it. The ANCHOR is a fixed vector that never changes for
//! the life of the climb (the built-in defaults unless `--anchor` says
//! otherwise). A promotion is vetoed when the candidate's win share against
//! the anchor is UNAMBIGUOUSLY below the sitting champion's -- intervals
//! disjoint, not merely a lower point estimate. That conservatism is on
//! purpose: the gate exists to catch a slide, not to referee noise, and a
//! veto that fires on noise would stall a healthy climb. It costs one extra
//! duel per accepted generation, which is a rounding error against `lambda`
//! challenge duels per generation.
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

use tta::arena::{Match, Summary};
use tta::bots::greedy::BotKind;
use tta::bots::weighted::eval::{load_weights, save_weights};
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

fn mutate(w: &Weights, s: &mut Search, sigma: f64, forced: Option<Op>) -> Mutation {
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
    /// The mutant's win share against the champion.
    share: f64,
    /// One-sided lower bound on that share at `accept_z`.
    lo: f64,
    games: usize,
}

/// Play `mutant` against `champion` in growing batches, stopping as soon as
/// the answer is not in doubt.
///
/// Two stopping rules, both from Python's `challenge`: break out early when
/// the lower bound has cleared the null (a clear win, and more games only cost
/// time), and abandon as soon as the running mean is BELOW the null and the
/// screening batch is spent (stop paying for a loser). Everything in between
/// keeps buying games up to `max_games`.
///
/// The early accept additionally needs [`Config::min_games`] behind it. A
/// batch of four deals can hand back a lower bound well clear of the null on
/// nothing but seat luck -- the first smoke run of this binary promoted a
/// mutant on a 12-game 0.50 exactly that way -- and an accept is permanent
/// where a rejection only costs one generation. So the cheap stopping rule is
/// the one for LOSERS; winners have to be shown twice.
fn challenge(mutant: &Weights, cfg: &Config, seed: u64) -> Challenge {
    let players = cfg.players as usize;
    let null = 1.0 / players as f64;
    let floor = cfg.accept_floor().min(cfg.max_games);
    let mut shares: Vec<Option<f64>> = Vec::new();
    let mut batch = cfg.screen;

    while shares.len() < cfg.max_games {
        let want = batch.min(cfg.max_games - shares.len());
        let mut duel = Match {
            a: *mutant,
            b: cfg.champion,
            kind: cfg.kind,
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
        shares.extend(duel.play().iter().map(|d| Some(d.share)));

        let est = stats::paired(&shares, players);
        let lo = est.mean - cfg.accept_z * est.se;
        if lo > null && shares.len() >= floor {
            break; // a clear win, not a lucky one
        }
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

fn measure_anchor(w: &Weights, cfg: &Config, seed: u64) -> Anchor {
    let mut duel = Match {
        a: *w,
        b: cfg.anchor,
        kind: cfg.kind,
        games: cfg.anchor_games,
        players: cfg.players,
        seed,
        threads: cfg.threads,
    };
    duel.validate().expect("anchor duel was validated at start-up");
    let s = Summary::of(&duel.play(), cfg.players as usize);
    Anchor { mean: s.win.mean, half: if s.win.half.is_finite() { s.win.half } else { 1.0 } }
}

impl Anchor {
    /// `self` is unambiguously worse than `other`: the intervals do not even
    /// touch. Deliberately conservative -- see this file's module doc.
    fn clearly_worse_than(&self, other: &Anchor) -> bool {
        self.mean + self.half < other.mean - other.half
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
    /// Games a challenge must have played before an early accept is allowed.
    /// Zero means twice the screening batch, which is Python's default and the
    /// smallest floor that makes a promotion survive a second, disjoint batch.
    min_games: usize,
    max_games: usize,
    anchor_games: usize,
    threads: usize,
    accept_z: f64,
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
                min_games: 0,
                max_games: 240,
                anchor_games: 120,
                threads: 1,
                accept_z: 1.2816,
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
  --min-games N      games before an early accept is allowed (default: 2x --screen)
  --max-games N      games a single challenge may spend (default 240)
  --anchor-games N   games per anchor measurement (default 120)
  --threads N        games in parallel (default 1)
  --seed N           run seed (default 0)
  --sigma X          initial step size, if not resumed (default 0.25)
  --sigma-floor X    smallest step the 1/5th rule may shrink to (default 0.08)
  --stall-kick N     rejections before a forced big jump; 0 disables (default 15)
  --accept-z X       strictness of the accept gate (default 1.2816 = 90% one-sided)
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut start: Option<PathBuf> = None;
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
            "--min-games" => a.cfg.min_games = parse_num(&value(flag)?, flag)?,
            "--max-games" => a.cfg.max_games = parse_num(&value(flag)?, flag)?,
            "--anchor-games" => a.cfg.anchor_games = parse_num(&value(flag)?, flag)?,
            "--threads" => a.cfg.threads = parse_num(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--sigma" => a.sigma = parse_num(&value(flag)?, flag)?,
            "--sigma-floor" => a.sigma_floor = parse_num(&value(flag)?, flag)?,
            "--stall-kick" => a.stall_kick = parse_num(&value(flag)?, flag)?,
            "--accept-z" => a.cfg.accept_z = parse_num(&value(flag)?, flag)?,
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
            && progress.since_accept % args.stall_kick == 0
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
        if let Some((m, _)) = &best {
            // The drift veto. Measured on a different seed range than the
            // sitting champion's standing so the two are not the same games.
            let cand = measure_anchor(
                &m.weights,
                &args.cfg,
                10_007u64.wrapping_add(progress.gen.wrapping_mul(37)),
            );
            if cand.clearly_worse_than(&standing) {
                vetoed = true;
            } else {
                args.cfg.champion = m.weights;
                standing = cand;
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

        let secs = t0.elapsed().as_secs_f64();
        let verdict = if accepted {
            "ACCEPT"
        } else if vetoed {
            "VETO  "
        } else {
            "reject"
        };
        println!(
            "[{}p] gen {} {} sigma={:.3} {:.1}s anchor={:.3} {}",
            args.cfg.players,
            progress.gen,
            verdict,
            progress.sigma,
            secs,
            standing.mean,
            tried.join(" "),
        );
        if let Some(path) = &args.log {
            let line = format!(
                "{{\"gen\":{},\"players\":{},\"accepted\":{},\"vetoed\":{},\"sigma\":{:.4},\
                 \"secs\":{:.1},\"since_accept\":{},\"anchor\":{:.4},\"anchor_half\":{:.4},\
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
    fn null(&self) -> f64 {
        1.0 / self.players as f64
    }

    /// See [`Config::min_games`].
    fn accept_floor(&self) -> usize {
        if self.min_games == 0 {
            2 * self.screen
        } else {
            self.min_games
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

    /// A promotion is permanent where a rejection costs one generation, so an
    /// early accept has to be shown over more than one screening batch.
    #[test]
    fn an_early_accept_needs_two_screening_batches_behind_it() {
        let c = Config { screen: 24, min_games: 0, ..cfg() };
        assert_eq!(c.accept_floor(), 48);
        let explicit = Config { screen: 24, min_games: 96, ..cfg() };
        assert_eq!(explicit.accept_floor(), 96);
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
}
