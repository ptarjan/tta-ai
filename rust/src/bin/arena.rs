//! `arena` -- duel two weight vectors and say whether one is actually better.
//!
//! ```text
//! arena --a challenger.json --b experiments/rust_champion_3p.json --games 240 --threads 6
//! ```
//!
//! The measurement itself lives in [`tta::arena`], which is also what the
//! `climb` binary decides on -- see that module for why the design is
//! seat-paired and why the interval clusters on the deal. This file is the
//! command line and the report, nothing else.
//!
//! # What it decides on
//!
//! The headline is A's win share against a null of `1 / players`, which is
//! what A would score if the two vectors were interchangeable. `verdict
//! accept` means the whole confidence interval clears that null, not merely
//! that the point estimate does.

use std::process::ExitCode;
use std::time::Instant;

use tta::arena::{Match, Summary};
use tta::bots::greedy::BotKind;
use tta::bots::weighted::eval::load_weights;
use tta::game::MOVE_CAP;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    /// The duel to play. `a` defaults to the built-in vector, so
    /// `arena --b x.json` asks the useful question "is the trained champion
    /// better than the defaults?" with no second file.
    duel: Match,
    name_a: String,
    name_b: String,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            duel: Match { games: 60, ..Match::new(3) },
            name_a: "defaults".to_string(),
            name_b: "defaults".to_string(),
        }
    }
}

const USAGE: &str = "\
usage: arena [options]

  --a PATH        challenger weights (default: the built-in vector)
  --b PATH        defender weights   (default: the built-in vector)
  --kind KIND     bot kind both sides play (default weighted)
  --games N       games; rounded DOWN to a whole number of deals (default 60)
  --players N     2, 3 or 4 (default 3)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
  --help

Exit status is 0 on a completed run whatever the verdict; it is non-zero only
if the run itself failed.  Read `verdict` for the result.
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--a" => {
                let p = value(flag)?;
                a.duel.a = load_weights(std::path::Path::new(&p))?;
                a.name_a = short_name(&p);
            }
            "--b" => {
                let p = value(flag)?;
                a.duel.b = load_weights(std::path::Path::new(&p))?;
                a.name_b = short_name(&p);
            }
            "--kind" => a.duel.kind = value(flag)?.parse::<BotKind>()?,
            "--games" => a.duel.games = parse_num(&value(flag)?, flag)?,
            "--players" => a.duel.players = parse_num::<u8>(&value(flag)?, flag)?,
            "--seed" => a.duel.seed = parse_num(&value(flag)?, flag)?,
            "--threads" => a.duel.threads = parse_num(&value(flag)?, flag)?,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    a.duel.validate()?;
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

/// A path's file stem, for the report's row labels. Purely cosmetic.
fn short_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

// ================================================================= reporting

fn report(args: &Args, s: &Summary, games: usize, elapsed: f64) {
    let null = args.duel.null();

    println!("games        {} ({} deals x {} seats)", games, s.win.n_deals, args.duel.players);
    println!("players      {}", args.duel.players);
    println!("kind         {}", args.duel.kind.name());
    println!("A            {}", args.name_a);
    println!("B            {}", args.name_b);
    println!("mean moves   {:.1}", s.mean_moves);
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", games as f64 / elapsed);
    if s.cap_hits > 0 {
        println!("WARNING      {} game(s) hit the {MOVE_CAP}-move cap -- that is a bug", s.cap_hits);
    }
    println!();

    println!(
        "win rate     {:.1}%  +/- {:.1}   (null {:.1}%)   p = {:.4}",
        100.0 * s.win.mean,
        100.0 * s.win.half,
        100.0 * null,
        s.win.p_against(null),
    );
    println!(
        "             [naive +/- {:.1}, rho {:+.2}, deff {:.2}, {} deals]",
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

    // The gate: the whole interval clear of the null, not just the point.
    if s.win.beats(null) {
        println!(
            "verdict      accept -- {} beats {} (interval clear of the null)",
            args.name_a, args.name_b
        );
    } else if s.win.hi() < null {
        println!("verdict      reject -- {} is WORSE than {}", args.name_a, args.name_b);
    } else {
        println!("verdict      inconclusive -- the interval still straddles the null");
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("arena: {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let duels = args.duel.play();
    let summary = Summary::of(&duels, args.duel.players as usize);
    report(&args, &summary, duels.len(), started.elapsed().as_secs_f64());
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A partial deal is exactly the seat-biased observation the design exists
    /// to exclude, so `--games` rounds down to whole deals.
    #[test]
    fn games_round_down_to_whole_deals() {
        let parsed = parse_args(&["--games".into(), "10".into(), "--players".into(), "3".into()])
            .unwrap()
            .unwrap();
        assert_eq!(parsed.duel.games, 9);
    }

    #[test]
    fn too_few_games_for_even_one_deal_is_an_error() {
        assert!(parse_args(&["--games".into(), "3".into(), "--players".into(), "4".into()]).is_err());
    }

    #[test]
    fn player_counts_outside_the_base_game_are_rejected() {
        assert!(parse_args(&["--players".into(), "5".into()]).is_err());
        assert!(parse_args(&["--players".into(), "1".into()]).is_err());
    }

    #[test]
    fn an_unknown_kind_is_rejected_before_any_game_is_played() {
        assert!(parse_args(&["--kind".into(), "not_a_bot".into()]).is_err());
    }
}
