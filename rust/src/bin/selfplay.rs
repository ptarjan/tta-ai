//! `selfplay` -- run bots against each other and report who won.
//!
//! This is the Rust side of what `experiments/` drives in Python today: deal a
//! game, hand each seat to a bot, play to §12.5, tally. Everything downstream
//! (the league, weight climbing, generating training positions for the neural
//! stack) is a loop around this binary, so it is deliberately small and its
//! output is deliberately machine-readable.
//!
//! ```text
//! selfplay --games 200 --players 3 --bots weighted,greedy,random --threads 6
//! ```
//!
//! # Seat bias is real, so seats rotate
//!
//! Seating order is not fair in Through the Ages: §1.9 hands the first player
//! one civil action and the last player four, and the whole of Age A is spent
//! paying that back. A run that pins `--bots a,b` to seats 0,1 is measuring
//! the seating rule as much as the bots. So game *g* rotates the spec by *g*:
//! over a run divisible by the number of distinct kinds, every kind plays
//! every seat an equal number of times. `--no-rotate` turns this off for the
//! rare case where you want a specific seat assignment held fixed.
//!
//! # Determinism
//!
//! Game *g* is played from seed `base_seed + g` with bots seeded from the same
//! number, so a rerun with the same arguments plays the same games in the same
//! order regardless of `--threads`. Threads take whole games, never plies.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, make_seats, BotKind};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};

// ====================================================================== args

/// Parsed command line. Every field has a default that produces a useful run,
/// so `selfplay` with no arguments is a smoke test rather than an error.
#[derive(Clone, Debug)]
struct Args {
    games: usize,
    players: u8,
    /// Comma-separated bot kinds, cycled round-robin over the seats by
    /// [`make_seats`]. Not split into a `Vec<BotKind>` here: `make_seats` owns
    /// the parse, and duplicating it would be exactly the two-registries bug
    /// this project keeps finding.
    bots: String,
    seed: u64,
    threads: usize,
    rotate: bool,
    /// Print a line per game as well as the summary.
    verbose: bool,
    /// The vector every evaluator seat plays. One vector for the whole table
    /// here on purpose: this binary measures a champion against other KINDS,
    /// and a champion against another VECTOR is the arena's job, not this
    /// one's.
    weights: Weights,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            games: 20,
            players: 3,
            bots: "weighted".to_string(),
            seed: 1,
            threads: 1,
            rotate: true,
            verbose: false,
            weights: Weights::default(),
        }
    }
}

const USAGE: &str = "\
usage: selfplay [options]

  --games N       games to play (default 20)
  --players N     2, 3 or 4 (default 3)
  --bots SPEC     comma-separated bot kinds, cycled over seats (default weighted)
  --seed N        base seed; game g uses seed+g (default 1)
  --weights PATH  champion JSON every evaluator seat plays (default: the
                  built-in defaults)
  --threads N     games in parallel (default 1)
  --no-rotate     pin the spec to seats instead of rotating it per game
  --verbose       print a line per game
  --list-bots     print the known bot kinds and exit
  --help
";

/// Hand-rolled because `[dependencies]` is empty on purpose (see
/// `Cargo.toml`): a clap dependency to read six flags is not a trade this
/// crate makes.
fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        // `value` is only forced for the flags that take one, so `--verbose`
        // does not eat the next flag.
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--games" => a.games = parse_num(&value(flag)?, flag)?,
            "--players" => a.players = parse_num::<u8>(&value(flag)?, flag)?,
            "--bots" => a.bots = value(flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--weights" => a.weights = load_weights(std::path::Path::new(&value(flag)?))?,
            "--threads" => a.threads = parse_num(&value(flag)?, flag)?,
            "--no-rotate" => a.rotate = false,
            "--verbose" | "-v" => a.verbose = true,
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
    // Fail on a bad spec here rather than inside a worker thread, where the
    // panic would be one of `threads` identical messages.
    make_seats(&a.bots, a.players, a.weights)?;
    Ok(Some(a))
}

fn parse_num<T: std::str::FromStr>(s: &str, flag: &str) -> Result<T, String> {
    s.parse().map_err(|_| format!("{flag}: {s:?} is not a number"))
}

// =================================================================== results

/// What one seat did in one game. A single collection of these is kept rather
/// than parallel `kinds`/`cultures`/`wins` vectors -- keeping those in step by
/// index is the shape that lets a tally silently describe the wrong bot.
#[derive(Clone, Copy, Debug)]
struct SeatOutcome {
    kind: BotKind,
    seat: u8,
    culture: i32,
    resigned: bool,
    /// 1.0 for a clean win, 1/n for an n-way tie, 0.0 otherwise. Shares sum
    /// to 1.0 per game, so a mean over games reads directly as a win rate.
    win_share: f64,
}

#[derive(Clone, Debug)]
struct GameOutcome {
    seed: u64,
    moves: usize,
    cap_hit: bool,
    seats: Vec<SeatOutcome>,
}

/// Rotate `spec`'s kinds left by `by` so seat *i* of game *g* gets a different
/// kind than seat *i* of game *g-1*. Returns a spec string rather than a
/// `Vec<BotKind>` so that [`make_bots`] stays the only thing that parses one.
fn rotated_spec(spec: &str, by: usize) -> String {
    let parts: Vec<&str> = spec.split(',').collect();
    let n = parts.len();
    (0..n).map(|i| parts[(i + by) % n]).collect::<Vec<_>>().join(",")
}

fn play_one(args: &Args, index: usize) -> GameOutcome {
    let seed = args.seed.wrapping_add(index as u64);
    let spec =
        if args.rotate { rotated_spec(&args.bots, index) } else { args.bots.clone() };
    // The spec was validated in `parse_args`; rotation only reorders it.
    let seats = make_seats(&spec, args.players, args.weights).expect("spec already validated");
    let mut bots = build_bots(&seats, seed as i64);

    let mut state = game::new_game(args.players, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, _legal| {
        bots[s.current as usize].pick(s)
    });

    let winners = game::winners(&state);
    let share = 1.0 / winners.len() as f64;
    let seats = (0..args.players)
        .map(|i| SeatOutcome {
            kind: bots[i as usize].kind(),
            seat: i,
            culture: state.players[i as usize].culture as i32,
            resigned: state.players[i as usize].resigned,
            win_share: if winners.contains(&i) { share } else { 0.0 },
        })
        .collect();

    GameOutcome { seed, moves: outcome.moves_played, cap_hit: outcome.move_cap_hit, seats }
}

// ================================================================= reporting

#[derive(Clone, Copy, Debug, Default)]
struct Tally {
    games: usize,
    win_share: f64,
    culture: i64,
    resigned: usize,
}

impl Tally {
    fn add(&mut self, s: &SeatOutcome) {
        self.games += 1;
        self.win_share += s.win_share;
        self.culture += i64::from(s.culture);
        self.resigned += usize::from(s.resigned);
    }

    fn win_rate(&self) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.win_share / self.games as f64
        }
    }

    fn mean_culture(&self) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.culture as f64 / self.games as f64
        }
    }
}

/// A run's headline numbers. Printed as aligned columns rather than JSON: the
/// consumers so far are a human reading a terminal and `sort`/`awk`.
fn report(args: &Args, games: &[GameOutcome], elapsed_secs: f64) {
    // `BTreeMap` for a stable print order that does not depend on which thread
    // finished first.
    let mut by_kind: BTreeMap<&'static str, Tally> = BTreeMap::new();
    let mut by_seat: BTreeMap<u8, Tally> = BTreeMap::new();
    let mut moves = 0usize;
    let mut capped = 0usize;

    for g in games {
        moves += g.moves;
        capped += usize::from(g.cap_hit);
        for s in &g.seats {
            by_kind.entry(s.kind.name()).or_default().add(s);
            by_seat.entry(s.seat).or_default().add(s);
        }
    }

    let n = games.len();
    println!("games        {n}");
    println!("players      {}", args.players);
    println!("bots         {}{}", args.bots, if args.rotate { " (rotated)" } else { " (pinned)" });
    println!("seeds        {}..{}", args.seed, args.seed + n as u64 - 1);
    println!("mean moves   {:.1}", moves as f64 / n as f64);
    println!("elapsed      {elapsed_secs:.1}s  ({:.1} games/s)", n as f64 / elapsed_secs.max(1e-9));
    if capped > 0 {
        // MOVE_CAP is two orders of magnitude above a real game, so this is a
        // loop in the engine, not a long game. Loud on purpose.
        println!("WARNING      {capped} game(s) hit the {MOVE_CAP}-move cap -- that is a bug, not a long game");
    }

    println!("\n{:<12} {:>6} {:>9} {:>10} {:>9}", "bot", "games", "win rate", "mean cult", "resigned");
    for (name, t) in &by_kind {
        println!(
            "{:<12} {:>6} {:>8.1}% {:>10.1} {:>9}",
            name,
            t.games,
            t.win_rate() * 100.0,
            t.mean_culture(),
            t.resigned
        );
    }

    println!("\n{:<12} {:>6} {:>9} {:>10}", "seat", "games", "win rate", "mean cult");
    for (seat, t) in &by_seat {
        println!(
            "{:<12} {:>6} {:>8.1}% {:>10.1}",
            seat,
            t.games,
            t.win_rate() * 100.0,
            t.mean_culture()
        );
    }
}

// ====================================================================== main

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("selfplay: {e}");
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let mut games: Vec<Option<GameOutcome>> = vec![None; args.games];

    // Each thread claims whole games off a shared counter. Games differ in
    // length by several times over -- a `plan` seat is far slower than a
    // `random` one -- so a static split would leave cores idle at the end.
    std::thread::scope(|scope| {
        let (slots, args, next) = (&mut games[..], &args, &next);
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(move || {
                let mut mine = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= args.games {
                        break;
                    }
                    let g = play_one(args, i);
                    if args.verbose {
                        println!(
                            "game {:>5} seed {:>10} moves {:>5}  {}",
                            i,
                            g.seed,
                            g.moves,
                            g.seats
                                .iter()
                                .map(|s| format!("{}:{}", s.kind.name(), s.culture))
                                .collect::<Vec<_>>()
                                .join(" ")
                        );
                    }
                    mine.push((i, g));
                }
                mine
            }));
        }
        // Results are written back by index so the report never depends on
        // completion order, which is what keeps a `--threads 6` run's output
        // identical to a `--threads 1` one.
        for h in handles {
            for (i, g) in h.join().expect("self-play thread panicked") {
                slots[i] = Some(g);
            }
        }
    });

    let games: Vec<GameOutcome> = games.into_iter().map(|g| g.expect("every game played")).collect();
    let capped = games.iter().any(|g| g.cap_hit);
    report(&args, &games, start.elapsed().as_secs_f64());
    if capped {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_gives_every_kind_every_seat() {
        // Two kinds over two games: seat 0 plays each kind once.
        assert_eq!(rotated_spec("a,b", 0), "a,b");
        assert_eq!(rotated_spec("a,b", 1), "b,a");
        assert_eq!(rotated_spec("a,b,c", 2), "c,a,b");
        // Rotating by the length is the identity, so a run divisible by the
        // number of kinds is balanced.
        assert_eq!(rotated_spec("a,b,c", 3), "a,b,c");
    }

    #[test]
    fn args_default_to_a_runnable_smoke_test() {
        let a = parse_args(&[]).unwrap().unwrap();
        assert_eq!(a.players, 3);
        assert!(a.games > 0);
        assert!(a.rotate);
    }

    #[test]
    fn a_bad_spec_fails_before_any_game_is_played() {
        // The regression this guards: an unknown kind used to surface as
        // `threads` identical panics from inside worker threads.
        let argv = ["--bots".to_string(), "weighted,nosuchbot".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    #[test]
    fn flags_without_values_do_not_eat_the_next_flag() {
        let argv =
            ["--verbose".to_string(), "--games".to_string(), "7".to_string()];
        let a = parse_args(&argv).unwrap().unwrap();
        assert!(a.verbose);
        assert_eq!(a.games, 7);
    }

    #[test]
    fn player_counts_outside_the_base_game_are_rejected() {
        for n in ["1", "5"] {
            assert!(parse_args(&["--players".to_string(), n.to_string()]).is_err());
        }
    }

    #[test]
    fn win_shares_sum_to_one_per_game() {
        let args = Args { games: 1, players: 3, bots: "random".to_string(), ..Args::default() };
        let g = play_one(&args, 0);
        let total: f64 = g.seats.iter().map(|s| s.win_share).sum();
        assert!((total - 1.0).abs() < 1e-9, "win shares summed to {total}");
        assert!(!g.cap_hit, "a random 3p game should end well inside the move cap");
    }

    #[test]
    fn the_same_seed_plays_the_same_game() {
        let args = Args { games: 1, players: 2, bots: "greedy".to_string(), ..Args::default() };
        let a = play_one(&args, 3);
        let b = play_one(&args, 3);
        assert_eq!(a.moves, b.moves);
        assert_eq!(
            a.seats.iter().map(|s| s.culture).collect::<Vec<_>>(),
            b.seats.iter().map(|s| s.culture).collect::<Vec<_>>()
        );
    }
}
