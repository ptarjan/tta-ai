//! `dump_selfplay` -- play self-play games with a champion weight vector and
//! write one [`tta::bots::neural::dump::DecisionRecord`] per decision point.
//!
//! ```text
//! dump_selfplay --games 4 --players 3 \
//!     --champion ../experiments/policy_seed/rust_champion_3p.json \
//!     --out /tmp/policy_dump.tpd
//! ```
//!
//! This is the data source a policy head trains against (`bots::neural::
//! action`/`bots::neural::dump`'s own doc comments): every seat at the
//! table plays the SAME champion vector (self-play by the current
//! champions, design decision 3 on the calling task -- never a human game),
//! and every decision is recorded from the ACTOR's own public viewpoint
//! (`state.decider()`, not `state.current` -- see `state.rs`'s doc comment
//! on why those differ for a pending decision like a defense or an
//! auction). The label is backfilled once the game actually ends, so it is
//! never available to leak into the state/action encodings themselves.
//!
//! `--champion` should point at a COPY of the live champion JSON, not the
//! live file under `experiments/` that `climb` rewrites every few minutes
//! (the calling task's own instruction) -- a long dump run would otherwise
//! silently change which vector it is playing partway through.

use std::path::PathBuf;
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Seat};
use tta::bots::neural::action::encode_action;
use tta::bots::neural::dump::{DecisionRecord, DumpWriter};
use tta::bots::neural::encode;
use tta::bots::weighted::eval::load_weights;
use tta::game::{self, MOVE_CAP};

#[derive(Clone, Debug)]
struct Args {
    games: usize,
    players: u8,
    seed: u64,
    out: PathBuf,
    /// Defaults to `../experiments/policy_seed/rust_champion_{players}p.json`
    /// -- relative to `rust/`, matching where this binary is built and run
    /// from (the calling task's own "cargo runs from `<clone>/rust`").
    champion: Option<PathBuf>,
    verbose: bool,
}

impl Default for Args {
    fn default() -> Args {
        Args { games: 4, players: 3, seed: 1, out: PathBuf::from("selfplay_dump.tpd"), champion: None, verbose: false }
    }
}

const USAGE: &str = "\
usage: dump_selfplay [options]

  --games N      games to play (default 4)
  --players N    2, 3 or 4 (default 3)
  --seed N       base seed; game g uses seed+g (default 1)
  --out PATH     dump file to append records to (default selfplay_dump.tpd)
  --champion PATH
                 champion weights JSON every seat plays (default:
                 ../experiments/policy_seed/rust_champion_{players}p.json --
                 a frozen COPY, never the live experiments/ file climb
                 rewrites -- see this binary's top doc comment)
  --verbose      print a line per game
  --help
";

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

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
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--out" => a.out = PathBuf::from(value(flag)?),
            "--champion" => a.champion = Some(PathBuf::from(value(flag)?)),
            "--verbose" | "-v" => a.verbose = true,
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
    Ok(Some(a))
}

fn champion_path(a: &Args) -> PathBuf {
    a.champion.clone().unwrap_or_else(|| {
        PathBuf::from(format!("../experiments/policy_seed/rust_champion_{}p.json", a.players))
    })
}

/// Play one game, recording every decision, and append the backfilled
/// records to `writer`. Returns how many records this game produced.
fn play_and_dump(
    players: u8,
    seed: u64,
    weights: tta::bots::weighted::weights::Weights,
    writer: &mut DumpWriter,
) -> Result<usize, String> {
    let seats = vec![Seat { kind: BotKind::Weighted, weights }; players as usize];
    let mut bots = build_bots(&seats, seed as i64);

    let mut pending: Vec<DecisionRecord> = Vec::new();
    let mut state = game::new_game(players, seed);
    let _outcome = game::play_game(&mut state, MOVE_CAP, |s, legal| {
        let actor = s.decider();
        let state_vec = encode::encode(s, actor);
        let legal_vecs: Vec<Vec<f64>> =
            legal.as_slice().iter().map(|&mv| encode_action(actor, mv)).collect();

        let mv = bots[actor as usize].pick(s);
        let chosen = legal
            .as_slice()
            .iter()
            .position(|&m| m == mv)
            .unwrap_or_else(|| panic!("bot picked {mv:?}, which is not in its own legal list"))
            as u32;

        pending.push(DecisionRecord {
            players: s.num_players,
            actor,
            state: state_vec,
            legal: legal_vecs,
            chosen,
            result: 0.0, // backfilled below, once the game has actually ended
        });
        mv
    });

    let winners = game::winners(&state);
    let share = 1.0 / winners.len() as f64;
    for rec in &mut pending {
        rec.result = if winners.contains(&rec.actor) { share } else { 0.0 };
    }
    let n = pending.len();
    for rec in &pending {
        writer.write_record(rec)?;
    }
    Ok(n)
}

fn run(args: &Args) -> Result<(), String> {
    let champ_path = champion_path(args);
    let weights = load_weights(&champ_path)
        .map_err(|e| format!("loading champion weights from {}: {e}", champ_path.display()))?;

    let mut writer = DumpWriter::create_or_append(&args.out)?;
    let start = Instant::now();
    let mut total_records = 0usize;
    for g in 0..args.games {
        let seed = args.seed.wrapping_add(g as u64);
        let n = play_and_dump(args.players, seed, weights, &mut writer)?;
        total_records += n;
        if args.verbose {
            println!("game {g:>4}  seed {seed:>8}  {n:>4} decisions");
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    println!(
        "wrote {total_records} decision records from {} games ({}p, champion {}) to {}  [{elapsed:.1}s]",
        args.games,
        args.players,
        champ_path.display(),
        args.out.display()
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&argv) {
        Ok(None) => std::process::ExitCode::SUCCESS,
        Ok(Some(args)) => match run(&args) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("dump_selfplay: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Err(e) => {
            eprintln!("dump_selfplay: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

