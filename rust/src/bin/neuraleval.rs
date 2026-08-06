//! `neuraleval` -- head-to-head play strength between two bot specs, where
//! either side may be a value-net checkpoint.
//!
//! ```text
//! neuraleval --a nplan:checkpoints/cand.ckpt,width=8,nodes=1200 \
//!            --b nplan:checkpoints/best_search.ckpt,width=8,nodes=1200 \
//!            --games 200 --players 2 --threads 6
//! ```
//!
//! Rust port of `experiments/neural_eval.py`, and the reason
//! `experiments/pool_summary.py` has no port: this binary plays the whole
//! match in one process across `--threads` workers, so there are no shards to
//! pool and the interval is clustered on the DEAL by `tta::stats::paired`
//! rather than reconstructed from six shard means. See
//! `tta::bots::neural::eval`'s top doc comment for the full argument.
//!
//! `arena` remains the binary for the hill climb's question -- two WEIGHT
//! VECTORS of one kind, which is what `climb` decides on. This one answers
//! the neural loop's question: two arbitrary POLICIES, one of which is a
//! checkpoint. The two share `tta::arena::Duel`/`Summary` and the same seat
//! rotation, and a test pins that they deal identical games.

use std::process::ExitCode;
use std::time::Instant;

use tta::bots::neural::eval::{Eval, Report};
use tta::bots::neural::spec::{Contender, Spec};
use tta::game::MOVE_CAP;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    a: String,
    b: String,
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            a: "weighted".to_string(),
            b: "weighted".to_string(),
            games: 200,
            players: 2,
            seed: 0,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: neuraleval [options]

  --a SPEC        challenger, seated one per game and rotated through every
                  seat (default weighted)
  --b SPEC        defender, seated in every other chair (default weighted)
  --games N       games; rounded DOWN to a whole number of deals (default 200)
  --players N     2, 3 or 4 (default 2)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
  --help

A SPEC is KIND[:PATH][,KEY=VALUE]...

  KIND    random, greedy, weighted, quiescent, plan  (PATH is a weights JSON)
          neural, nplan                              (PATH is a checkpoint)
  KEY     width=  beam width                 (plan, nplan)
          nodes=  apply-call cap per decision (nplan)
          det=    determinize at the root, 0/1 (plan, neural, nplan)
          etb=    end-turn score bias        (neural)
          war=    price a declared war       (plan, nplan)

A KEY the KIND does not read is an ERROR, not a no-op: a silently ignored
width= measures a different bot than the one you asked for.

  neuraleval --a nplan:cand.ckpt,width=8 --b nplan:best.ckpt,width=8 --games 200
  neuraleval --a nplan:cand.ckpt,width=8 --b plan:champion_2p.json,width=8

Exit status is 0 on a completed run whatever the result; it is non-zero only
if the run itself failed.  Read the SUMMARY line.
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--a" => a.a = value(flag)?,
            "--b" => a.b = value(flag)?,
            "--games" => a.games = parse_num(&value(flag)?, flag)?,
            "--players" => a.players = parse_num::<u8>(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--threads" => a.threads = parse_num(&value(flag)?, flag)?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    // Parse BOTH specs before either checkpoint is read: a typo on the
    // command line should cost nothing, and a half-loaded match is not a
    // state worth reaching. See `bots::neural::spec`'s top doc comment.
    Spec::parse(&a.a).map_err(|e| format!("--a {e}"))?;
    Spec::parse(&a.b).map_err(|e| format!("--b {e}"))?;
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

// ================================================================= reporting

fn report(args: &Args, r: &Report, elapsed: f64) {
    let s = &r.summary;
    let null = r.null();
    println!("games        {} ({} deals x {} seats)", s.win.n_games, s.win.n_deals, args.players);
    println!("players      {}", args.players);
    println!("A            {}", args.a);
    println!("B            {}", args.b);
    println!("mean moves   {:.1}", s.mean_moves);
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", s.win.n_games as f64 / elapsed);
    if s.cap_hits > 0 {
        println!("WARNING      {} game(s) hit the {MOVE_CAP}-move cap -- that is a bug", s.cap_hits);
    }
    println!();
    println!(
        "win rate     {:.1}%  +/- {:.1}   (null {:.1}%)   p = {:.4}",
        100.0 * r.win(),
        100.0 * r.half(),
        100.0 * null,
        s.win.p_against(null),
    );
    println!(
        "             [se {:.4}, naive +/- {:.1}, rho {:+.2}, deff {:.2}, {} deals]",
        r.se(),
        100.0 * s.win.naive_half,
        s.win.rho,
        s.win.deff,
        s.win.n_deals,
    );
    println!(
        "culture      A {:.1}   best other {:.1}   lead {:+.1} +/- {:.1}",
        s.mean_culture_a, s.mean_culture_best_other, s.lead.mean, s.lead.half,
    );
    println!();
    // The one line a shell driver parses. Printed last so a reader's eye and
    // a `tail -1` land on the same thing.
    println!("{}", r.summary_line());
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("neuraleval: {e}");
            return ExitCode::FAILURE;
        }
    };

    let load = |what: &str, text: &str| -> Result<Contender, String> {
        Contender::parse_and_load(text).map_err(|e| format!("{what} {e}"))
    };
    let (a, b) = match (load("--a", &args.a), load("--b", &args.b)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("neuraleval: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut duel = Eval { games: args.games, seed: args.seed, threads: args.threads, ..Eval::new(&a, &b, args.players) };
    if let Err(e) = duel.validate() {
        eprintln!("neuraleval: {e}");
        return ExitCode::FAILURE;
    }

    let started = Instant::now();
    let r = duel.run();
    report(&args, &r, started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bad_spec_is_rejected_before_any_checkpoint_is_read() {
        assert!(parse_args(&["--a".into(), "mcts".into()]).is_err());
        assert!(parse_args(&["--b".into(), "weighted,width=8".into()]).is_err());
    }

    #[test]
    fn defaults_are_a_runnable_two_player_mirror() {
        let a = parse_args(&[]).unwrap().unwrap();
        assert_eq!(a.players, 2);
        assert_eq!(a.a, a.b);
    }

    #[test]
    fn flags_without_values_do_not_eat_the_next_flag() {
        let argv = ["--games".to_string(), "40".to_string(), "--threads".to_string(), "3".to_string()];
        let a = parse_args(&argv).unwrap().unwrap();
        assert_eq!(a.games, 40);
        assert_eq!(a.threads, 3);
    }

    #[test]
    fn an_unknown_flag_is_rejected() {
        assert!(parse_args(&["--device".into(), "cuda".into()]).is_err());
    }
}
