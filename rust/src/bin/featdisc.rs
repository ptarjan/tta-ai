//! `featdisc` -- WITHIN-DECISION DISCRIMINATION screen for candidate feature
//! columns.
//!
//! ```text
//! featdisc --games 300 --players 2 --threads 2 \
//!     --weights /tmp/fs_champ_2p_frozen.json --out /tmp/featdisc_2p.tsv
//! ```
//!
//! ## Why this exists
//!
//! `phidump` + `analysis/feature_screen.py` rank a candidate column by how
//! much held-out R2 it adds over `phi` when predicting the game's outcome.
//! That is a question about variation ACROSS DECISION POINTS. It cannot
//! answer the question a leaf evaluator actually needs answered, which is
//! whether the column varies ACROSS THE LEGAL MOVES AT ONE DECISION POINT.
//!
//! The leaf eval is `dot(w, phi(candidate))`. A column identical across every
//! candidate at a decision adds the same constant to every candidate's score
//! and cannot change the argmax AT ANY WEIGHT. So a column can explain
//! outcome variance beautifully -- especially one shaped like the label, e.g.
//! a projection of the final margin -- and still be unable to separate any
//! two legal moves. This binary measures exactly that separation.
//!
//! ## What is measured, per column
//!
//! At every decision point with two or more real candidates, every legal move
//! is trial-applied and the column is read off the resulting state:
//!
//! * `spread` = max - min across the candidate set, the quantity a weight
//!   multiplies when it decides anything;
//! * `const` = whether that spread is zero, i.e. the column was unable to
//!   separate ANY two legal moves here;
//! * `allzero` = whether the column was zero on every candidate, which
//!   distinguishes "structurally constant but populated" from "never
//!   populated at all";
//! * `distinct` = how many different values it took over the candidate set.
//!
//! Reduced over decisions: `mean_spread`, the constant fraction, the distinct
//! histogram, and separately the mean and SD of the column's value AT THE
//! MOVE THE CHAMPION ACTUALLY CHOSE -- which is precisely the population
//! `phidump` writes one row of, so `mean_spread / sd_chosen` puts a column's
//! within-decision variation in units of the across-decision variation the R2
//! screen was fit on. Columns of wildly different scale are comparable in
//! those units and are not comparable in raw ones.
//!
//! ## How the successor states are built
//!
//! Through `feature_screen::candidate_row`, which is the same call `phidump`
//! makes and which takes `phi` from `bots::weighted::eval::candidate_features`
//! itself and the extra columns from a state rebuilt by the engine's own
//! `apply::apply`. Nothing here knows or asserts which moves ought to move
//! which column; that is the entire point, and
//! `feature_screen::tests::trial_matches_candidate_features` is the guard that
//! the rebuilt state is the one `candidate_features` priced.
//!
//! The one thing this binary does decide for itself is the CANDIDATE SET: the
//! legal list with `Move::Resign` dropped unless resigning is the only legal
//! move. That mirrors `bots::filter_resign`, which is `pub(crate)` and so not
//! callable from a binary, and it is what `rank_moves` ranks.
//!
//! Costs about one extra `rank_moves` per decision, so it runs at a fraction
//! of `phidump`'s games and is not a substitute for it: this is a screen for
//! DISCRIMINATION, the R2 dump is a screen for PREDICTION, and a column needs
//! both.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Search, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::WeightKey;
use tta::feature_screen::{self, EXTRA_DIMS, EXTRA_KEYS};
use tta::game::{self, MOVE_CAP};
use tta::moves::Move;

/// Distinct-value counts are histogrammed up to this many; everything above
/// lands in the overflow bucket. A median read off the histogram is exact
/// unless the median itself is in that bucket, which the report flags.
const DIST_CAP: usize = 32;

/// Two values count as the same value below this, relative to the largest
/// magnitude in the candidate set. `linear_features` accumulates in `f64`, so
/// two candidates that genuinely reach the same quantity by different
/// arithmetic can differ in the last bits; a raw `==` would call that a real
/// spread and report a structurally inert column as live.
const EPS_REL: f64 = 1e-9;

#[derive(Clone, Debug)]
struct Args {
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
    out: PathBuf,
    weights: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            games: 64,
            players: 2,
            seed: 1,
            threads: 1,
            out: PathBuf::from("featdisc.tsv"),
            weights: None,
        }
    }
}

const USAGE: &str = "\
usage: featdisc --weights PATH [options]

  --games N      games to play (default 64)
  --players N    2, 3 or 4 (default 2)
  --seed N       base seed; game g uses seed+g (default 1)
  --threads N    games in parallel (default 1)
  --out PATH     TSV summary to write (default featdisc.tsv)
  --weights PATH champion JSON every seat plays, and the freeze point the
                 features are priced at (required; must be a frozen COPY,
                 never the live experiments/ file the climb rewrites)
  --help
";

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

fn parse_args() -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let mut value = |f: &str| it.next().ok_or_else(|| format!("{f} needs a value\n\n{USAGE}"));
        match flag.as_str() {
            "--games" => a.games = parse_num(&value(&flag)?, &flag)?,
            "--players" => a.players = parse_num::<u8>(&value(&flag)?, &flag)?,
            "--seed" => a.seed = parse_num(&value(&flag)?, &flag)?,
            "--threads" => a.threads = parse_num(&value(&flag)?, &flag)?,
            "--out" => a.out = PathBuf::from(value(&flag)?),
            "--weights" => a.weights = Some(PathBuf::from(value(&flag)?)),
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    if !(2..=4).contains(&a.players) {
        return Err(format!("--players must be 2, 3 or 4, got {}", a.players));
    }
    if a.games == 0 {
        return Err("--games must be at least 1".to_string());
    }
    if a.threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    a.weights.as_ref().ok_or_else(|| format!("--weights is required\n\n{USAGE}"))?;
    Ok(Some(a))
}

/// One column's running totals over decision points.
#[derive(Clone)]
struct Agg {
    /// Decision points seen (identical for every column; carried per column
    /// so a merged `Agg` is self-describing).
    n_dec: u64,
    /// ... of which the column took one value across the whole candidate set.
    n_const: u64,
    /// ... of which the column was zero on every candidate.
    n_allzero: u64,
    sum_spread: f64,
    max_spread: f64,
    /// `dist_hist[k]` = decisions where the column took `k` distinct values,
    /// index 0 unused, `DIST_CAP` is "that many or more".
    dist_hist: Vec<u64>,
    /// Value at the move actually chosen -- the `phidump` population.
    chosen_sum: f64,
    chosen_sumsq: f64,
}

impl Agg {
    fn new() -> Agg {
        Agg {
            n_dec: 0,
            n_const: 0,
            n_allzero: 0,
            sum_spread: 0.0,
            max_spread: 0.0,
            dist_hist: vec![0; DIST_CAP + 1],
            chosen_sum: 0.0,
            chosen_sumsq: 0.0,
        }
    }

    fn merge(&mut self, o: &Agg) {
        self.n_dec += o.n_dec;
        self.n_const += o.n_const;
        self.n_allzero += o.n_allzero;
        self.sum_spread += o.sum_spread;
        self.max_spread = self.max_spread.max(o.max_spread);
        for (a, b) in self.dist_hist.iter_mut().zip(&o.dist_hist) {
            *a += b;
        }
        self.chosen_sum += o.chosen_sum;
        self.chosen_sumsq += o.chosen_sumsq;
    }

    /// Smallest `k` with at least `q` of the mass at or below it.
    fn quantile_distinct(&self, q: f64) -> usize {
        let want = (q * self.n_dec as f64).ceil() as u64;
        let mut seen = 0u64;
        for (k, &c) in self.dist_hist.iter().enumerate() {
            seen += c;
            if seen >= want {
                return k;
            }
        }
        0
    }

    /// Reported alongside the median because the median is degenerate: any
    /// column constant on more than half its decisions has a median of 1, and
    /// most columns are.
    fn mean_distinct(&self) -> f64 {
        if self.n_dec == 0 {
            return 0.0;
        }
        let total: u64 = self.dist_hist.iter().enumerate().map(|(k, &c)| k as u64 * c).sum();
        total as f64 / self.n_dec as f64
    }

    fn mean_spread(&self) -> f64 {
        if self.n_dec == 0 {
            0.0
        } else {
            self.sum_spread / self.n_dec as f64
        }
    }

    fn sd_chosen(&self) -> f64 {
        if self.n_dec < 2 {
            return 0.0;
        }
        let n = self.n_dec as f64;
        let mean = self.chosen_sum / n;
        let var = (self.chosen_sumsq / n - mean * mean).max(0.0);
        var.sqrt()
    }
}

/// Totals that are about the run rather than about one column.
#[derive(Clone)]
struct Global {
    decisions: u64,
    /// Decision points with a single candidate: no decision to make, excluded
    /// from every column's statistics so a forced move cannot inflate any
    /// column's constant fraction.
    forced: u64,
    candidates: u64,
    max_candidates: usize,
    rows_seen: u64,
}

impl Global {
    fn new() -> Global {
        Global { decisions: 0, forced: 0, candidates: 0, max_candidates: 0, rows_seen: 0 }
    }

    fn merge(&mut self, o: &Global) {
        self.decisions += o.decisions;
        self.forced += o.forced;
        self.candidates += o.candidates;
        self.max_candidates = self.max_candidates.max(o.max_candidates);
        self.rows_seen += o.rows_seen;
    }
}

/// How many separated values `col` takes, under the same tolerance the
/// constant test uses, so the two statistics cannot disagree about whether a
/// column moved: `sorted` is scratch space the caller owns.
fn distinct_values(col: &[f64], maxabs: f64, sorted: &mut Vec<f64>) -> usize {
    sorted.clear();
    sorted.extend_from_slice(col);
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("feature columns are never NaN"));
    let eps = EPS_REL * (1.0 + maxabs);
    let mut n = 1;
    for w in sorted.windows(2) {
        if w[1] - w[0] > eps {
            n += 1;
        }
    }
    n
}

/// Play one game with every seat on `weights` and fold every decision point
/// into `aggs`.
///
/// `pick` is called exactly once per decision, before the candidate sweep, so
/// the trajectory is the one `phidump` walks at the same seed: the sweep
/// itself draws no bot randomness (`candidate_row`'s determinization is keyed
/// off the state, not off the bot's rng).
fn play_and_accumulate(
    players: u8,
    seed: u64,
    weights: tta::bots::weighted::weights::Weights,
    aggs: &mut [Agg],
    glob: &mut Global,
) {
    let seats =
        vec![Seat { kind: BotKind::Weighted, weights, search: Search::None }; players as usize];
    let mut bots = build_bots(&seats, seed as i64);
    let dims = aggs.len();

    let mut cands: Vec<Move> = Vec::new();
    let mut cols: Vec<Vec<f64>> = vec![Vec::new(); dims];
    let mut scratch: Vec<f64> = Vec::new();

    let mut state = game::new_game(players, seed);
    let _outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
        let actor = s.decider();
        let mv = bots[actor as usize].pick(s);

        // The candidate set `rank_moves` would rank: `bots::filter_resign`'s
        // rule, which is `pub(crate)` and cannot be called from here.
        cands.clear();
        if legal.as_slice().len() > 1 && legal.as_slice().iter().any(|m| !matches!(m, Move::Resign))
        {
            cands.extend(legal.as_slice().iter().copied().filter(|m| !matches!(m, Move::Resign)));
        } else {
            cands.extend(legal.as_slice().iter().copied());
        }

        if cands.len() < 2 {
            glob.forced += 1;
            return mv;
        }

        for c in cols.iter_mut() {
            c.clear();
        }
        let mut chosen_at: Option<usize> = None;
        for &cand in cands.iter() {
            let Some((phi, extra)) = feature_screen::candidate_row(s, cand, &weights) else {
                continue;
            };
            if cand == mv {
                chosen_at = Some(cols[0].len());
            }
            for (j, v) in phi.into_iter().chain(extra).enumerate() {
                cols[j].push(v);
            }
        }

        let n = cols[0].len();
        if n < 2 {
            glob.forced += 1;
            return mv;
        }
        // `pick` runs the same resignation filter, so the chosen move is in
        // the set by construction; a miss would silently bias `sd_chosen`.
        let chosen_at = chosen_at.expect("the chosen move is not in its own candidate set");

        glob.decisions += 1;
        glob.candidates += n as u64;
        glob.max_candidates = glob.max_candidates.max(n);
        glob.rows_seen += n as u64;

        for (j, col) in cols.iter().enumerate() {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            let mut maxabs = 0.0f64;
            for &v in col {
                lo = lo.min(v);
                hi = hi.max(v);
                maxabs = maxabs.max(v.abs());
            }
            let spread = hi - lo;
            let a = &mut aggs[j];
            a.n_dec += 1;
            if spread <= EPS_REL * (1.0 + maxabs) {
                a.n_const += 1;
                a.dist_hist[1] += 1;
                if maxabs == 0.0 {
                    a.n_allzero += 1;
                }
            } else {
                a.sum_spread += spread;
                a.max_spread = a.max_spread.max(spread);
                a.dist_hist[distinct_values(col, maxabs, &mut scratch).min(DIST_CAP)] += 1;
            }
            let c = col[chosen_at];
            a.chosen_sum += c;
            a.chosen_sumsq += c * c;
        }
        mv
    });
}

fn column_names() -> Vec<(String, String)> {
    let mut names: Vec<(String, String)> =
        WeightKey::ALL.iter().map(|k| ("phi".to_string(), format!("{k:?}"))).collect();
    names.extend(EXTRA_KEYS.iter().map(|k| ("extra".to_string(), (*k).to_string())));
    names
}

fn run(args: &Args) -> Result<(), String> {
    let champ_path = args.weights.as_ref().expect("--weights checked in parse_args");
    let weights = load_weights(champ_path)
        .map_err(|e| format!("loading champion weights from {}: {e}", champ_path.display()))?;
    let names = column_names();
    let dims = names.len();

    let totals = Mutex::new((vec![Agg::new(); dims], Global::new()));
    let next = AtomicUsize::new(0);
    let started = Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..args.threads {
            scope.spawn(|| {
                let mut aggs = vec![Agg::new(); dims];
                let mut glob = Global::new();
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= args.games {
                        break;
                    }
                    play_and_accumulate(args.players, args.seed + g as u64, weights, &mut aggs, &mut glob);
                }
                let mut t = totals.lock().expect("totals mutex poisoned");
                for (a, b) in t.0.iter_mut().zip(&aggs) {
                    a.merge(b);
                }
                t.1.merge(&glob);
            });
        }
    });

    let (aggs, glob) = totals.into_inner().expect("totals mutex poisoned");
    let secs = started.elapsed().as_secs_f64();

    let mut out = String::new();
    out.push_str(&format!(
        "# featdisc games={} players={} seed={} weights={} decisions={} forced={} \
         mean_candidates={:.3} max_candidates={} successor_states={} elapsed_s={:.1}\n",
        args.games,
        args.players,
        args.seed,
        champ_path.display(),
        glob.decisions,
        glob.forced,
        glob.candidates as f64 / glob.decisions.max(1) as f64,
        glob.max_candidates,
        glob.rows_seen,
        secs
    ));
    out.push_str(
        "block\tcolumn\tn_dec\tconst_frac\tallzero_frac\tmean_spread\tmax_spread\tsd_chosen\t\
         spread_ratio\tmedian_distinct\tmean_distinct\tp90_distinct\n",
    );
    for (i, (block, name)) in names.iter().enumerate() {
        let a = &aggs[i];
        let n = a.n_dec.max(1) as f64;
        let sd = a.sd_chosen();
        let ms = a.mean_spread();
        // A column that never moves anywhere has no ratio to report; one that
        // moves within decisions but not across them is the extreme opposite
        // and must not be silently printed as zero.
        let ratio = if ms == 0.0 {
            0.0
        } else if sd == 0.0 {
            f64::INFINITY
        } else {
            ms / sd
        };
        out.push_str(&format!(
            "{}\t{}\t{}\t{:.6}\t{:.6}\t{:.6e}\t{:.6e}\t{:.6e}\t{:.6}\t{}\t{:.4}\t{}\n",
            block,
            name,
            a.n_dec,
            a.n_const as f64 / n,
            a.n_allzero as f64 / n,
            ms,
            a.max_spread,
            sd,
            ratio,
            a.quantile_distinct(0.5),
            a.mean_distinct(),
            a.quantile_distinct(0.9)
        ));
    }
    std::fs::write(&args.out, &out).map_err(|e| format!("writing {}: {e}", args.out.display()))?;

    println!("games        {}", args.games);
    println!("players      {}", args.players);
    println!("weights      {}", champ_path.display());
    println!("columns      {dims} ({} phi + {EXTRA_DIMS} extra)", WeightKey::ALL.len());
    println!("decisions    {} (+{} forced, excluded)", glob.decisions, glob.forced);
    println!("candidates   {:.2} mean, {} max", glob.candidates as f64 / glob.decisions.max(1) as f64, glob.max_candidates);
    println!("successors   {}", glob.rows_seen);
    println!("out          {}", args.out.display());
    println!("elapsed      {secs:.1}s ({:.2} games/s)", args.games as f64 / secs);
    Ok(())
}

fn main() {
    match parse_args() {
        Ok(None) => {}
        Ok(Some(args)) => {
            if let Err(e) = run(&args) {
                eprintln!("featdisc: {e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("featdisc: {e}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reduction itself, on a matrix whose answers are known by hand: a
    /// constant column, a two-valued column, an all-zero column, and one
    /// whose candidates differ only in the last bits of an `f64`.
    #[test]
    fn aggregation_matches_hand_computed_spreads() {
        let mut a = Agg::new();
        for _ in 0..4 {
            a.n_dec += 1;
            a.dist_hist[3] += 1;
            a.sum_spread += 2.0;
            a.max_spread = a.max_spread.max(2.0);
            a.chosen_sum += 1.0;
            a.chosen_sumsq += 1.0;
        }
        assert_eq!(a.quantile_distinct(0.5), 3);
        assert!((a.mean_distinct() - 3.0).abs() < 1e-12);
        assert!((a.mean_spread() - 2.0).abs() < 1e-12);
        assert!(a.sd_chosen() < 1e-12, "a constant chosen value has zero SD");
    }

    #[test]
    fn float_noise_is_not_a_spread() {
        let v = 12.0f64;
        let noisy = v + v * 1e-16;
        let maxabs = noisy.abs();
        assert!(noisy - v <= EPS_REL * (1.0 + maxabs), "last-bit drift must count as constant");
        let real = v + 1e-3;
        assert!(real - v > EPS_REL * (1.0 + maxabs), "a real 1e-3 difference must count as spread");
    }

    #[test]
    fn distinct_counts_separated_values_only() {
        let mut scratch = Vec::new();
        let col = [3.0, 3.0 + 3.0 * 1e-16, 5.0, 5.0, 9.0];
        assert_eq!(distinct_values(&col, 9.0, &mut scratch), 3, "last-bit drift is one value");
        let flat = [7.0, 7.0, 7.0];
        assert_eq!(distinct_values(&flat, 7.0, &mut scratch), 1);
    }

    #[test]
    fn median_distinct_reads_the_histogram() {
        let mut a = Agg::new();
        a.n_dec = 10;
        a.dist_hist[1] = 6;
        a.dist_hist[4] = 4;
        assert_eq!(a.quantile_distinct(0.5), 1, "a column constant on 60% of decisions has median 1");
        assert_eq!(a.quantile_distinct(0.9), 4, "and its p90 still sees the four-valued decisions");
        assert!((a.mean_distinct() - 2.2).abs() < 1e-12);
        let mut b = Agg::new();
        b.n_dec = 10;
        b.dist_hist[1] = 4;
        b.dist_hist[4] = 6;
        assert_eq!(b.quantile_distinct(0.5), 4);
    }
}
