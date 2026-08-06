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
//! Reuses `bots::greedy`'s `make_seats`/`build_bots`/`Bot` for the teacher --
//! the same dispatch `selfplay.rs` uses -- rather than a second bot-kind
//! registry, and the same scoped-thread-over-an-atomic-counter shape
//! `selfplay.rs` uses for `--threads`, here fanning games into a shared
//! `Mutex<ShardWriter>` instead of a results slice (shards need to be built
//! incrementally across games, not just tallied at the end).

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tta::bots::greedy::{make_seats, BotKind, Seat};
use tta::bots::neural::encode::ENCODING_DIM;
use tta::bots::neural::rankdata::{play_and_record, FlushedShard, ShardWriter};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    games: usize,
    players: u8,
    /// Comma-separated bot kinds, cycled over seats by `make_seats` --
    /// exactly `selfplay.rs --bots`'s shape, so a mixed-teacher table (e.g.
    /// `plan,weighted`) is possible even though `neural_rankdata.py` never
    /// offered it (every seat there was the same `BookBot` version).
    teacher: String,
    /// Recorded verbatim into every shard's `teacher` tag; see
    /// `rankdata.rs`'s "vacuity hazard" doc section for why.
    teacher_tag: String,
    weights: Weights,
    stride: usize,
    krej: usize,
    epsilon: f64,
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
            teacher_tag: "weighted".to_string(),
            weights: Weights::default(),
            stride: 3,
            krej: 6,
            epsilon: 0.05,
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
  --teacher SPEC   comma-separated bot kinds, cycled over seats, the labels'
                   source (default weighted; --list-bots to see all kinds --
                   `plan` is the strongest available and what
                   experiments/plan_teacher_gen.py bootstraps from, at
                   higher CPU cost per game)
  --weights PATH   champion JSON every evaluator seat's teacher reads
                   (default: this build's built-in defaults)
  --stride N       sample every Nth ply (default 3)
  --krej N         rejected sibling children kept per sampled decision
                   (default 6)
  --epsilon F      exploration on the PLAYED move only, for state diversity;
                   the recorded label is always the teacher's preferred move
                   at the state actually visited (default 0.05)
  --seed0 N        base seed; game g uses seed0+g (default 0)
  --out PREFIX     output path prefix; shards are PREFIX.NNNN.rkd (default
                   rankdata/rk)
  --shard N        rows per shard, applied independently to the pair buffer
                   and the value buffer (default 120000)
  --threads N      games in parallel (default 1)
  --list-bots      print the known bot kinds and exit
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut weights_path: Option<String> = None;
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--games" => a.games = parse_num(&value(flag)?, flag)?,
            "--players" => a.players = parse_num::<u8>(&value(flag)?, flag)?,
            "--teacher" => a.teacher = value(flag)?,
            "--weights" => {
                let p = value(flag)?;
                a.weights = load_weights(std::path::Path::new(&p))?;
                weights_path = Some(p);
            }
            "--stride" => a.stride = parse_num(&value(flag)?, flag)?,
            "--krej" => a.krej = parse_num(&value(flag)?, flag)?,
            "--epsilon" => a.epsilon = parse_num(&value(flag)?, flag)?,
            "--seed0" => a.seed0 = parse_num(&value(flag)?, flag)?,
            "--out" => a.out = PathBuf::from(value(flag)?),
            "--shard" => a.shard = parse_num(&value(flag)?, flag)?,
            "--threads" => a.threads = parse_num(&value(flag)?, flag)?,
            "--list-bots" => {
                for k in BotKind::ALL {
                    println!("{}", k.name());
                }
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
    // Fail on a bad spec here, not inside a worker thread -- selfplay.rs's
    // identical reasoning: a spec error should be one message, not
    // `--threads` copies of the same panic.
    make_seats(&a.teacher, a.players, a.weights)?;

    a.teacher_tag = match weights_path {
        Some(p) => format!("{}@{p}", a.teacher),
        None => a.teacher.clone(),
    };

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

    let seats: Vec<Seat> = match make_seats(&args.teacher, args.players, args.weights) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rankdata: {e}");
            return ExitCode::FAILURE;
        }
    };

    let writer =
        Mutex::new(ShardWriter::new(args.out.clone(), args.teacher_tag.clone(), args.shard));
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let start = Instant::now();
    let failed = Mutex::new(None::<String>);

    std::thread::scope(|scope| {
        for _ in 0..threads {
            let (seats, writer, next, args, failed) = (&seats, &writer, &next, &args, &failed);
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
                let (pairs, values) = play_and_record(
                    seats,
                    args.players,
                    args.seed0 + g as u64,
                    args.stride,
                    args.krej,
                    args.epsilon,
                );
                let mut w = writer.lock().unwrap();
                let result = w.push_game(pairs, values);
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

    println!(
        "DONE games={} pairs={} vals={} dim={} teacher={}  ({:.1}s)",
        args.games,
        w.total_pairs(),
        w.total_values(),
        ENCODING_DIM,
        args.teacher_tag,
        start.elapsed().as_secs_f64()
    );
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_default_to_a_runnable_smoke_test() {
        let a = parse_args(&[]).unwrap().unwrap();
        assert_eq!(a.players, 2);
        assert!(a.games > 0);
        assert_eq!(a.teacher_tag, "weighted");
    }

    #[test]
    fn a_bad_teacher_spec_fails_before_any_game_is_played() {
        let argv = ["--teacher".to_string(), "nosuchbot".to_string()];
        assert!(parse_args(&argv).is_err());
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
