//! `kindmatch` -- duel two BOT KINDS (not two weight vectors) at the same
//! table, on shared deals, playing the SAME weights on both sides.
//!
//! Built for one question `arena`/`climb` cannot answer: their `Match` takes
//! a single `kind` field that both `a` and `b` play (see
//! `tta::arena::Match`), because their whole point is comparing two VECTORS
//! of the same kind. Comparing two KINDS -- does `QuiescentBot`'s lookahead
//! actually buy anything over `WeightedBot`'s one-ply eval, weights held
//! fixed -- needed a duel where `a` and `b` differ in kind instead. This is
//! that duel, with the same seat-pairing `arena::Match::play_one` uses so the
//! comparison is on shared deals, not lucky seats.
//!
//! ```text
//! kindmatch --a quiescent --b weighted --weights champ.json --games 480 \
//!     --players 3 --threads 6
//! ```

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};
use tta::stats;

#[derive(Clone, Debug)]
struct Args {
    a: BotKind,
    b: BotKind,
    weights: Weights,
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            a: BotKind::Weighted,
            b: BotKind::Weighted,
            weights: Weights::defaults(),
            games: 60,
            players: 3,
            seed: 0,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: kindmatch --a KIND --b KIND [options]

  --a KIND        challenger bot kind, seated one per game
  --b KIND        defender bot kind, seated in every other chair
  --weights PATH  weight vector BOTH sides play (default: built-in defaults)
  --games N       games; rounded down to a whole number of deals (default 60)
  --players N     2, 3 or 4 (default 3)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
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
            "--a" => a.a = value(flag)?.parse::<BotKind>()?,
            "--b" => a.b = value(flag)?.parse::<BotKind>()?,
            "--weights" => a.weights = load_weights(std::path::Path::new(&value(flag)?))?,
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
    if !(2..=4).contains(&a.players) {
        return Err(format!("--players must be 2, 3 or 4, got {}", a.players));
    }
    if a.threads == 0 {
        return Err("--threads must be at least 1".to_string());
    }
    let per_deal = a.players as usize;
    a.games -= a.games % per_deal;
    if a.games == 0 {
        return Err(format!("--games must be at least {per_deal} at {}p", a.players));
    }
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

/// Game `index`: A in seat `index % players`, deal `seed + index / players`
/// -- identical scheme to `arena::Match::play_one`, so a run here is on the
/// same deals a `Match` at these `--seed`/`--players` would use.
fn play_one(args: &Args, index: usize) -> f64 {
    let players = args.players as usize;
    let seat = index % players;
    let seed = (args.seed.wrapping_add((index / players) as u64))
        .wrapping_mul(7919)
        .wrapping_add(17);

    let seats: Vec<Seat> = (0..players)
        .map(|i| Seat {
            kind: if i == seat { args.a } else { args.b },
            weights: args.weights,
        })
        .collect();
    let mut bots = build_bots(&seats, seed as i64);

    let mut state = game::new_game(args.players, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, _legal| bots[s.current as usize].pick(s));
    if outcome.move_cap_hit {
        eprintln!("kindmatch: WARNING game at seed {seed} hit the {MOVE_CAP}-move cap");
    }

    let winners = game::winners(&state);
    if winners.contains(&(seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("kindmatch: {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<(usize, f64)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..args.threads)
            .map(|_| {
                let (next, args) = (&next, &args);
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= args.games {
                            return mine;
                        }
                        mine.push((index, play_one(args, index)));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });
    let mut slots: Vec<Option<f64>> = vec![None; args.games];
    for (i, share) in done.into_iter().flatten() {
        slots[i] = Some(share);
    }
    let elapsed = started.elapsed().as_secs_f64();

    let shares: Vec<Option<f64>> = slots;
    let est = stats::paired(&shares, args.players as usize);
    let null = 1.0 / args.players as f64;

    println!("games        {} ({} games/s)", args.games, args.games as f64 / elapsed);
    println!("players      {}", args.players);
    println!("A (rotates)  {}", args.a.name());
    println!("B (rest)     {}", args.b.name());
    println!("elapsed      {elapsed:.1}s");
    println!();
    println!(
        "A win rate   {:.2}%  +/- {:.2}   (null {:.2}%)   p = {:.4}",
        100.0 * est.mean,
        100.0 * est.half,
        100.0 * null,
        est.p_against(null),
    );
    if est.beats(null) {
        println!("verdict      accept -- {} beats {} (interval clear of the null)", args.a.name(), args.b.name());
    } else if est.hi() < null {
        println!("verdict      reject -- {} is WORSE than {}", args.a.name(), args.b.name());
    } else {
        println!("verdict      inconclusive -- the interval still straddles the null");
    }
    ExitCode::SUCCESS
}
