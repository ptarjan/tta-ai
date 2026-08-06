//! `rankdata` -- generate value-net training shards (ranking pairs + value
//! anchors) from Rust self-play. Rust port of `experiments/neural_rankdata.py`;
//! see `rust/src/bots/neural/rankdata.rs`'s top doc comment for the objective,
//! the shard format, and the one behavioural fix carried over from
//! `experiments/plan_teacher_gen.py` (determinized sibling children, so a
//! `end_turn` candidate never leaks the true next card).
//!
//! ```text
//! rankdata --games 800 --players 2 --teacher weighted --stride 3 --krej 6 \
//!     --epsilon 0.05 --out rankdata/rk --shard 120000 --threads 6
//! ```
//!
//! The teacher is a `bots::neural::spec` spec, so it may be a CHECKPOINT
//! (`nplan:best_search.ckpt,width=8`) and not only a classical bot -- which
//! is what makes this one binary cover both of the loop's generation stages:
//! the bootstrap from the frozen linear champion (`plan:champion_2p.json,
//! width=8`, `experiments/plan_teacher_gen.py`) and the per-iteration
//! search-backed self-play (`nplan:`, `experiments/neural_gen_plan.py`).
//!
//! Uses the same scoped-thread-over-an-atomic-counter shape `selfplay.rs`
//! uses for `--threads`, here fanning games into a shared
//! `Mutex<ShardWriter>` instead of a results slice (shards need to be built
//! incrementally across games, not just tallied at the end).
//!
//! ## The DONE line
//!
//! ```text
//! DONE games=240 pairs=41233 vals=17904 dim=1906 teacher=nplan:best.ckpt,width=8 \
//!      values=search-leaves sampled=4021 overruled=2263 DISAGREE=0.5628  (612.4s)
//! ```
//!
//! `DISAGREE` is the loop's health meter and is `NA`, never `0`, when the
//! teacher has no net to take a 1-ply argmax with -- see
//! `bots::neural::rankdata`'s top doc comment for why that distinction is
//! load-bearing rather than pedantic.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tta::bots::greedy::BotKind;
use tta::bots::neural::encode::ENCODING_DIM;
use tta::bots::neural::rankdata::{play_and_record, FlushedShard, Recorded, ShardWriter, ValueSource};
use tta::bots::neural::spec::{Contender, Spec};

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    games: usize,
    players: u8,
    /// One `bots::neural::spec` spec, seated in EVERY chair -- and recorded
    /// verbatim into every shard's `teacher` tag; see `rankdata.rs`'s
    /// "vacuity hazard" doc section for why the tag matters.
    ///
    /// Not the comma-separated mixed-kind table `selfplay --bots` takes:
    /// commas belong to the knob grammar here (`plan:w.json,width=8`), and
    /// nothing has ever wanted a mixed-teacher table anyway
    /// (`neural_rankdata.py` seated the same `BookBot` in every chair).
    teacher: String,
    stride: usize,
    krej: usize,
    epsilon: f64,
    /// Value rows kept per searched decision. A width-8 beam prices a few
    /// hundred positions per decision; keeping them all would swamp the
    /// ranking pairs and correlate the batch.
    leaf_per_decision: usize,
    seed0: u64,
    out: PathBuf,
    shard: usize,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            games: 800,
            players: 2,
            teacher: "weighted".to_string(),
            stride: 3,
            krej: 6,
            epsilon: 0.05,
            leaf_per_decision: 12,
            seed0: 0,
            out: PathBuf::from("rankdata/rk"),
            shard: 120_000,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: rankdata [options]

  --games N        games to play (default 800)
  --players N      2, 3 or 4 (default 2)
  --teacher SPEC   the bot whose choices are the labels, seated in every
                   chair (default weighted).  KIND[:PATH][,KEY=VALUE]...
                   e.g. plan:analysis/frozen/champion_2p.json,width=8
                        nplan:checkpoints/best_search.ckpt,width=8,nodes=1200
                   A teacher with a beam (plan/nplan) also supplies the
                   value rows from the leaves it actually priced; one
                   without a beam falls back to pre-move states and the DONE
                   line says so.
  --stride N       sample every Nth ply (default 3)
  --krej N         rejected sibling children kept per sampled decision
                   (default 6)
  --leaf-per-decision N
                   value rows kept per searched decision (default 12)
  --epsilon F      exploration on the PLAYED move only, for state diversity;
                   the recorded label is always the teacher's preferred move
                   at the state actually visited (default 0.05)
  --seed0 N        base seed; game g uses seed0+g (default 0)
  --out PREFIX     output path prefix; shards are PREFIX.NNNN.rkd (default
                   rankdata/rk)
  --shard N        rows per shard, applied independently to the pair buffer
                   and the value buffer (default 120000)
  --threads N      games in parallel (default 1)
  --list-bots      print the known classical bot kinds and exit
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--games" => a.games = parse_num(&value(flag)?, flag)?,
            "--players" => a.players = parse_num::<u8>(&value(flag)?, flag)?,
            "--teacher" => a.teacher = value(flag)?,
            "--stride" => a.stride = parse_num(&value(flag)?, flag)?,
            "--krej" => a.krej = parse_num(&value(flag)?, flag)?,
            "--leaf-per-decision" => a.leaf_per_decision = parse_num(&value(flag)?, flag)?,
            "--epsilon" => a.epsilon = parse_num(&value(flag)?, flag)?,
            "--seed0" => a.seed0 = parse_num(&value(flag)?, flag)?,
            "--out" => a.out = PathBuf::from(value(flag)?),
            "--shard" => a.shard = parse_num(&value(flag)?, flag)?,
            "--threads" => a.threads = parse_num(&value(flag)?, flag)?,
            "--list-bots" => {
                for k in BotKind::ALL {
                    println!("{}", k.name());
                }
                println!("neural   (needs neural:CKPT)");
                println!("nplan    (needs nplan:CKPT)");
                return Ok(None);
            }
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
    if a.stride == 0 {
        return Err("--stride must be at least 1".to_string());
    }
    if a.leaf_per_decision == 0 {
        return Err("--leaf-per-decision must be at least 1".to_string());
    }
    // Fail on a bad spec here, not inside a worker thread -- selfplay.rs's
    // identical reasoning: a spec error should be one message, not
    // `--threads` copies of the same panic. Parsing is pure string work, so
    // this rejects a typo without reading a checkpoint.
    Spec::parse(&a.teacher)?;

    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

// ====================================================================== main

fn report_flush(f: &FlushedShard) {
    println!("  wrote {}  pairs={} vals={}", f.path.display(), f.pairs, f.values);
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rankdata: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Some(parent) = args.out.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("rankdata: {}: {e}", parent.display());
                return ExitCode::FAILURE;
            }
        }
    }

    let teacher = match Contender::parse_and_load(&args.teacher) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rankdata: {e}");
            return ExitCode::FAILURE;
        }
    };

    let writer = Mutex::new(ShardWriter::new(args.out.clone(), args.teacher.clone(), args.shard));
    // Totals that are not the writer's business, behind their own lock so a
    // thread that only has rows to add never blocks on them.
    let health = Mutex::new(Health::default());
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let start = Instant::now();
    let failed = Mutex::new(None::<String>);

    std::thread::scope(|scope| {
        for _ in 0..threads {
            let (teacher, writer, next, args, failed, health) =
                (&teacher, &writer, &next, &args, &failed, &health);
            scope.spawn(move || loop {
                if failed.lock().unwrap().is_some() {
                    return;
                }
                let g = next.fetch_add(1, Ordering::Relaxed);
                if g >= args.games {
                    return;
                }
                // Play the whole game OUTSIDE the writer's lock -- games are
                // the expensive part (a `plan` teacher especially so) and
                // rows are cheap to hand off, so the lock is only ever held
                // for the push below. Claiming one game at a time off a
                // shared counter (not a static split) is selfplay.rs's same
                // work-stealing shape, for the same reason: teacher game
                // lengths vary several-fold, so a static split would leave
                // some threads idle at the end.
                let recorded = play_and_record(
                    teacher,
                    args.players,
                    args.seed0 + g as u64,
                    args.stride,
                    args.krej,
                    args.epsilon,
                    args.leaf_per_decision,
                );
                health.lock().unwrap().fold(&recorded);
                let mut w = writer.lock().unwrap();
                let result = w.push_game(recorded.pairs, recorded.values);
                let (tp, tv) = (w.total_pairs(), w.total_values());
                drop(w);
                match result {
                    Ok(flushed) => {
                        for f in &flushed {
                            report_flush(f);
                        }
                    }
                    Err(e) => {
                        *failed.lock().unwrap() = Some(e);
                        return;
                    }
                }
                if (g + 1) % 50 == 0 {
                    println!("game {}/{}  pairs {tp}  vals {tv}", g + 1, args.games);
                }
            });
        }
    });

    if let Some(e) = failed.into_inner().unwrap() {
        eprintln!("rankdata: {e}");
        return ExitCode::FAILURE;
    }

    let mut w = writer.into_inner().unwrap();
    match w.finish() {
        Ok(Some(f)) => report_flush(&f),
        Ok(None) => {}
        Err(e) => {
            eprintln!("rankdata: {e}");
            return ExitCode::FAILURE;
        }
    }

    let health = health.into_inner().unwrap();
    println!(
        "DONE games={} pairs={} vals={} dim={} teacher={} values={} sampled={} overruled={} DISAGREE={}  ({:.1}s)",
        args.games,
        w.total_pairs(),
        w.total_values(),
        ENCODING_DIM,
        args.teacher,
        health.values_from.name(),
        health.sampled,
        health.overruled.map(|n| n.to_string()).unwrap_or_else(|| "NA".to_string()),
        health.disagree_rate(),
        start.elapsed().as_secs_f64()
    );
    ExitCode::SUCCESS
}

/// The two numbers a caller has to see that are not rows: how often the
/// teacher's search overruled the net's own argmax, and where the value rows
/// came from.
#[derive(Debug)]
struct Health {
    sampled: u64,
    /// `None` until a game reports one, and `None` FOREVER if the teacher has
    /// no net -- see `bots::neural::rankdata::Recorded::overruled`.
    overruled: Option<u64>,
    values_from: ValueSource,
}

impl Default for Health {
    fn default() -> Health {
        Health { sampled: 0, overruled: None, values_from: ValueSource::PreMoveState }
    }
}

impl Health {
    fn fold(&mut self, r: &Recorded) {
        self.sampled += r.sampled;
        if let Some(n) = r.overruled {
            self.overruled = Some(self.overruled.unwrap_or(0) + n);
        }
        if r.values_from == ValueSource::SearchLeaves {
            self.values_from = ValueSource::SearchLeaves;
        }
    }

    /// The health meter itself. `NA` when there is no net to compare against
    /// and when nothing was sampled: `docs/NEURAL_SEARCH_LOOP.md` 6 makes
    /// `DISAGREE < 0.02` a pre-registered kill condition, so printing 0.0000
    /// for "could not measure" would ask the driver to stop a run for a
    /// reason that was never observed.
    fn disagree_rate(&self) -> String {
        match self.overruled {
            None => "NA".to_string(),
            Some(_) if self.sampled == 0 => "NA".to_string(),
            Some(n) => format!("{:.4}", n as f64 / self.sampled as f64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_default_to_a_runnable_smoke_test() {
        let a = parse_args(&[]).unwrap().unwrap();
        assert_eq!(a.players, 2);
        assert!(a.games > 0);
        assert_eq!(a.teacher, "weighted");
    }

    #[test]
    fn a_bad_teacher_spec_fails_before_any_game_is_played() {
        let argv = ["--teacher".to_string(), "nosuchbot".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn a_checkpoint_teacher_spec_parses() {
        let argv = ["--teacher".to_string(), "nplan:best.ckpt,width=8,nodes=1200".to_string()];
        assert_eq!(parse_args(&argv).unwrap().unwrap().teacher, "nplan:best.ckpt,width=8,nodes=1200");
    }

    /// A teacher with no net can only ABSTAIN on the vacuity meter; the
    /// kill condition is `DISAGREE < 0.02`, so reporting 0.0000 here would
    /// ask a driver to halt a run over a measurement nobody made.
    #[test]
    fn the_disagree_rate_is_na_when_there_was_no_net_to_compare_against() {
        let h = Health { sampled: 100, overruled: None, ..Health::default() };
        assert_eq!(h.disagree_rate(), "NA");
    }

    #[test]
    fn the_disagree_rate_is_na_when_nothing_was_sampled() {
        let h = Health { sampled: 0, overruled: Some(0), ..Health::default() };
        assert_eq!(h.disagree_rate(), "NA");
    }

    #[test]
    fn the_disagree_rate_is_the_overruled_share_of_sampled_decisions() {
        let h = Health { sampled: 200, overruled: Some(113), ..Health::default() };
        assert_eq!(h.disagree_rate(), "0.5650");
    }

    /// One game finding leaves is enough to make the whole run's rows leaf
    /// rows; the report must not be dragged back by games that sampled none.
    #[test]
    fn any_game_reporting_search_leaves_makes_the_run_report_search_leaves() {
        let mut h = Health::default();
        h.fold(&Recorded { pairs: vec![], values: vec![], values_from: ValueSource::PreMoveState, sampled: 3, overruled: Some(1) });
        h.fold(&Recorded { pairs: vec![], values: vec![], values_from: ValueSource::SearchLeaves, sampled: 5, overruled: Some(2) });
        assert_eq!(h.values_from, ValueSource::SearchLeaves);
        assert_eq!(h.sampled, 8);
        assert_eq!(h.overruled, Some(3));
    }

    #[test]
    fn flags_without_values_do_not_eat_the_next_flag() {
        let argv = ["--epsilon".to_string(), "0.2".to_string(), "--games".to_string(), "7".to_string()];
        let a = parse_args(&argv).unwrap().unwrap();
        assert!((a.epsilon - 0.2).abs() < 1e-12);
        assert_eq!(a.games, 7);
    }

    #[test]
    fn stride_of_zero_is_rejected_rather_than_dividing_by_it_later() {
        let argv = ["--stride".to_string(), "0".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn player_counts_outside_the_base_game_are_rejected() {
        for n in ["1", "5"] {
            assert!(parse_args(&["--players".to_string(), n.to_string()]).is_err());
        }
    }
}
