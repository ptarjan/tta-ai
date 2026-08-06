//! `harness` -- play the CGE app, log the bot.
//!
//! ```text
//! harness --players 3 --difficulty hard --app-version 2.4.1
//! harness fields --players 3 --seat 0
//! ```
//!
//! The first form runs a real operator session (`harness::play`): a
//! per-round checksum on our own board, a minimal derived prompt for
//! opponents, and a structured JSONL record (`harness::record`) with the
//! setup and pact bias attached. The `fields` subcommand instead prints the
//! pre-flight table of what a human must type and what they must not
//! (`harness::fields::report`), the Rust equivalent of Python's
//! `python3 -m harness.fields`.
//!
//! Ported from `harness/play.py`'s `main`/`build_parser` (the session) and
//! `harness/fields.py`'s `_main` (the `fields` subcommand), folded into one
//! binary the way `bin/advisor.rs` is the command line over `tta::advisor`:
//! the mirror, the checksum logic and the record format all live in the
//! library so they can be unit-tested without a terminal attached; this
//! file is the argument parsing, the real stdin/stdout, and nothing else.

use std::path::PathBuf;
use std::process::ExitCode;

use tta::advisor::advisor::{self as advisor, Advisor};
use tta::advisor::state_io;
use tta::harness::fields;
use tta::harness::mirror;
use tta::harness::play::{self, HarnessConsole, Io};
use tta::harness::record::{Difficulty, GameLog, Setup};
use tta::state::ROW_SIZE;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    players: u8,
    seat: u8,
    seed: u64,
    weights: Option<PathBuf>,
    difficulty: Difficulty,
    free: bool,
    dlc: Option<bool>,
    app_version: String,
    operator: String,
    personalities: Vec<String>,
    id: Option<String>,
    log: Option<PathBuf>,
    load: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            players: 3,
            seat: 0,
            seed: 0,
            weights: None,
            difficulty: Difficulty::Hard,
            free: false,
            dlc: None,
            app_version: String::new(),
            operator: String::new(),
            personalities: Vec::new(),
            id: None,
            log: None,
            load: None,
        }
    }
}

const USAGE: &str = "\
usage: harness [options]
       harness fields [--players N] [--seat N]

  --players N       2, 3 or 4 (default 3)
  --seat N          your seat, 0 = start player (default 0)
  --seed N          deal seed, only used for a fresh game (default 0)
  --weights PATH    bot weight JSON (default: built-in champion/defaults)
  --difficulty D    training | easy | medium | hard (default hard)
  --free            free mode: you play, the bot only watches
  --dlc BOOL         New Leaders & Wonders on? MUST be off (asked if omitted)
  --app-version V   CGE app version, recorded in the log
  --operator NAME   who is playing, recorded in the log
  --personality P   a 'world leader' AI personality (repeatable; labels the
                     game as a different experiment)
  --id ID           game id (default: a timestamp)
  --log PATH        where to write the JSONL record (default games/<id>.jsonl)
  --load PATH       resume from a snapshot file instead of dealing fresh
  --help

`fields` subcommand prints the derived field list and exits; no game is
played.
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> { it.next().cloned().ok_or_else(|| format!("{flag} needs a value")) };
        match flag.as_str() {
            "--players" => a.players = parse_num(&value(flag)?, flag)?,
            "--seat" => a.seat = parse_num(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--weights" => a.weights = Some(PathBuf::from(value(flag)?)),
            "--difficulty" => a.difficulty = Difficulty::parse(&value(flag)?)?,
            "--free" => a.free = true,
            "--dlc" => {
                let v = value(flag)?.to_lowercase();
                a.dlc = Some(matches!(v.as_str(), "1" | "true" | "yes"));
            }
            "--app-version" => a.app_version = value(flag)?,
            "--operator" => a.operator = value(flag)?,
            "--personality" => a.personalities.push(value(flag)?),
            "--id" => a.id = Some(value(flag)?),
            "--log" => a.log = Some(PathBuf::from(value(flag)?)),
            "--load" => a.load = Some(PathBuf::from(value(flag)?)),
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
    if a.seat >= a.players {
        return Err(format!("--seat must be 0..{}, got {}", a.players - 1, a.seat));
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

// ======================================================== real stdin/stdout

struct StdIo;

impl Io for StdIo {
    fn ask(&mut self, prompt: &str, _adv: &Advisor) -> String {
        use std::io::Write;
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => "quit".to_string(), // EOF
            Ok(_) => line.trim_end_matches(['\n', '\r']).to_string(),
            Err(_) => "quit".to_string(),
        }
    }

    fn say(&mut self, s: &str) {
        println!("{s}");
    }
}

// ------------------------------------------------------------------- setup

fn ask_setup_stdin(args: &Args) -> Result<Setup, String> {
    play::ask_setup(
        |prompt| {
            use std::io::Write;
            print!("{prompt}");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            line.trim_end_matches(['\n', '\r']).to_string()
        },
        |s| println!("{s}"),
        args.id.clone(),
        args.players,
        args.seat,
        args.difficulty,
        args.free,
        args.dlc,
        args.app_version.clone(),
        args.weights.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        args.operator.clone(),
        args.personalities.clone(),
    )
}

fn build_board(args: &Args) -> Result<state_io::Board, String> {
    match &args.load {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            state_io::loads(&text)
        }
        None => Ok(state_io::new_board(args.players, args.seat, args.seed)),
    }
}

fn run_session(args: &Args) -> ExitCode {
    let setup = match ask_setup_stdin(args) {
        Ok(s) => s,
        Err(e) => {
            println!("\nrefusing to start: {e}");
            return ExitCode::from(2);
        }
    };

    let board = match build_board(args) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (bot, source) = advisor::load_bot(board.state.num_players, args.weights.as_deref());
    let mut adv = Advisor::new(board, bot, source);
    if args.load.is_none() {
        adv.dealt_slots = (0..ROW_SIZE).collect();
    }

    let verdicts = fields::probe_position(adv.state(), args.seat, &adv.bot.weights, fields::PROBES);
    let requirements = fields::requirements(&verdicts, fields::PROBES);
    let path = args.log.clone().unwrap_or_else(|| PathBuf::from(format!("games/{}.jsonl", setup.game_id)));
    let log = match GameLog::new(setup, Some(&path), &requirements) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("\nlogging to {}", path.display());
    let mut console = HarnessConsole::new(adv, log, StdIo, !args.free, verdicts);
    console.run();
    ExitCode::SUCCESS
}

// --------------------------------------------------------------- `fields`

fn run_fields_report(argv: &[String]) -> ExitCode {
    let mut players: u8 = 3;
    let mut seat: u8 = 0;
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> { it.next().cloned().ok_or_else(|| format!("{flag} needs a value")) };
        let res = match flag.as_str() {
            "--players" => value(flag).and_then(|v| parse_num::<u8>(&v, flag)).map(|v| players = v),
            "--seat" => value(flag).and_then(|v| parse_num::<u8>(&v, flag)).map(|v| seat = v),
            "--help" | "-h" => {
                println!("usage: harness fields [--players N] [--seat N]");
                return ExitCode::SUCCESS;
            }
            other => Err(format!("unknown flag {other}")),
        };
        if let Err(e) = res {
            eprintln!("harness fields: {e}");
            return ExitCode::FAILURE;
        }
    }

    let (reqs, cost) = fields::report(players, seat, None);
    println!("derived field set, {players}p, seat {seat}\n");
    println!("{:<28} {:<7} {:<26} why", "observable", "verdict", "ask");
    println!("{}", "-".repeat(100));
    for r in &reqs {
        let mark = if r.mandatory() { "*" } else { " " };
        println!("{mark}{:<27} {:<7} {:<26} {}", r.probe.id.as_str(), r.verdict.as_str(), r.probe.ask, r.reason);
    }
    let mandatory = reqs.iter().filter(|r| r.mandatory()).count();
    println!("\n* = the human must transcribe it.  {mandatory} of {} observables.", reqs.len());
    println!("estimated transcription: {cost:.0} s/round (~{:.0} min over a 20-round game)", cost * 20.0 / 60.0);
    let _ = mirror::SPINE; // keep the import meaningful if the report format changes
    ExitCode::SUCCESS
}

// ------------------------------------------------------------------- main

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.first().map(String::as_str) == Some("fields") {
        return run_fields_report(&argv[1..]);
    }
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("harness: {e}");
            return ExitCode::FAILURE;
        }
    };
    run_session(&args)
}
