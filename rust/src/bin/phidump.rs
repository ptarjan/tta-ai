//! `phidump` -- play self-play games with a champion weight vector and write
//! one raw LINEAR FEATURE VECTOR per decision point, labelled with how that
//! game actually ended for the acting seat.
//!
//! ```text
//! phidump --games 300 --players 2 --threads 5 \
//!     --weights /tmp/champ_2p_frozen.json --out /tmp/phi_2p.bin
//! ```
//!
//! This exists to answer one question: is the champion's leaf evaluation
//! losing anything by being LINEAR in `phi`? The champion scores a position
//! as `dot(w, phi(state, w))` (`bots::weighted::eval::dot`), so the honest
//! control for "an MLP on `phi`" is not the champion's own dot product --
//! which was never fit to predict an outcome -- but the BEST LINEAR
//! predictor of the same label from the same `phi`. A nonlinear head only
//! earns a trial if it beats that. This binary emits exactly the data both
//! sides of that comparison need, and nothing else: it does not train, fit
//! or judge anything.
//!
//! Two properties the vectors here inherit from `candidate_features`, both
//! load-bearing:
//!
//! 1. `phi` DEPENDS ON `w` (eleven identity-aware coordinates are priced at
//!    a `freeze` vector -- see `eval.rs`'s own section comment). So `--weights`
//!    is both the vector every seat plays AND the freeze point, and the dump
//!    is only meaningful at that one point in weight space.
//! 2. The vector is built by `candidate_features` rather than by calling
//!    `linear_features` on a post-move state directly, so the root clone, the
//!    event determinization, the shared `RivalContext` and the `EndTurnBias`
//!    indicator are all exactly what `rank_moves` itself would have used. A
//!    hand-rolled version of that machinery is how a dump silently stops
//!    describing the bot it came from.
//!
//! `--weights` must be a frozen COPY. The live `experiments/rust_champion_*.json`
//! are rewritten by the running climb every few minutes, which would change
//! both the players and the freeze point partway through a run.
//!
//! ## On-disk format
//!
//! Little-endian throughout. A 16-byte header -- magic `TPHI`, `u32` version
//! (1), `u32` dims, `u32` zero -- then one fixed-width record per decision:
//!
//! ```text
//! u32 game_id | u8 players | u8 actor | u16 round | f32 margin | f32 win_share | f32[dims] phi
//! ```
//!
//! `margin` is the actor's final culture minus the best of the others (a tie
//! for the lead is 0 for both, matching `bots::neural::rankdata`'s own
//! label); `win_share` is `1/|winners|` for a winner and 0 otherwise, matching
//! `dump_selfplay`'s. Both are backfilled after game over, so neither can
//! leak into the features. `dims` is `WeightKey::ALL.len()`, and the sidecar
//! `<out>.keys` written next to the dump lists those key names in order --
//! a reader that skips it is one `WeightKey` addition away from silently
//! misaligned columns.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Search, Seat};
use tta::bots::weighted::eval::{candidate_features, load_weights};
use tta::bots::weighted::weights::WeightKey;
use tta::game::{self, MOVE_CAP};

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
        Args { games: 32, players: 2, seed: 1, threads: 1, out: PathBuf::from("phi_dump.bin"), weights: None }
    }
}

const USAGE: &str = "\
usage: phidump --weights PATH [options]

  --games N      games to play (default 32)
  --players N    2, 3 or 4 (default 2)
  --seed N       base seed; game g uses seed+g (default 1)
  --threads N    games in parallel (default 1)
  --out PATH     dump file to write (default phi_dump.bin); a sidecar
                 <out>.keys lists the feature column names in order
  --weights PATH champion JSON every seat plays, and the freeze point the
                 features are priced at (required; must be a frozen COPY,
                 never the live experiments/ file climb rewrites)
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

/// Same construction as `dump_selfplay`'s: `players` and `seed` combined so a
/// corpus assembled from separate 2p/3p/4p runs cannot collide, as long as no
/// single run's `--seed` reaches eight digits.
fn game_id(players: u8, seed: u64) -> u32 {
    (players as u32) * 10_000_000 + (seed % 10_000_000) as u32
}

/// One decision point, held until the game ends and the label is known.
struct Row {
    actor: u8,
    round: u16,
    phi: Vec<f64>,
}

/// Play one game and return its rows, already labelled. Every seat plays
/// `weights`, and the features are frozen at that same vector.
fn play_and_collect(
    players: u8,
    seed: u64,
    weights: tta::bots::weighted::weights::Weights,
) -> (Vec<Row>, Vec<f32>, Vec<f32>) {
    let seats = vec![Seat { kind: BotKind::Weighted, weights, search: Search::None }; players as usize];
    let mut bots = build_bots(&seats, seed as i64);

    let mut rows: Vec<Row> = Vec::new();
    let mut state = game::new_game(players, seed);
    let _outcome = game::play_game(&mut state, MOVE_CAP, |s, _legal| {
        let actor = s.decider();
        let round = s.round;
        let mv = bots[actor as usize].pick(s);
        // The chosen move only -- `candidate_features` over the whole legal
        // list would cost a full `rank_moves` a second time, and this dump
        // is about the position reached, not about the ranking.
        //
        // `allow_resign: false` matches `rank_moves`' own default; a picked
        // `Move::Resign` is therefore filtered out and yields no row, which
        // is correct -- a resignation is not a position anyone evaluates.
        if let Some((_, phi)) = candidate_features(s, &[mv], false, &weights).into_iter().next() {
            rows.push(Row { actor, round, phi });
        }
        mv
    });

    let scores = game::scores(&state);
    let winners = game::winners(&state);
    let share = 1.0 / winners.len() as f32;
    // Margin against the best OTHER seat, so a tied lead is 0 for both --
    // the same label `bots::neural::rankdata` already uses.
    let margins: Vec<f32> = (0..players as usize)
        .map(|i| {
            let best_other = (0..players as usize).filter(|&j| j != i).map(|j| scores[j]).max().unwrap_or(0);
            (scores[i] - best_other) as f32
        })
        .collect();
    let shares: Vec<f32> =
        (0..players).map(|i| if winners.contains(&i) { share } else { 0.0 }).collect();
    (rows, margins, shares)
}

fn encode_game(gid: u32, players: u8, rows: &[Row], margins: &[f32], shares: &[f32]) -> Vec<u8> {
    let dims = WeightKey::ALL.len();
    let mut buf = Vec::with_capacity(rows.len() * (12 + 4 * dims));
    for r in rows {
        buf.extend_from_slice(&gid.to_le_bytes());
        buf.push(players);
        buf.push(r.actor);
        buf.extend_from_slice(&r.round.to_le_bytes());
        buf.extend_from_slice(&margins[r.actor as usize].to_le_bytes());
        buf.extend_from_slice(&shares[r.actor as usize].to_le_bytes());
        for &v in &r.phi {
            buf.extend_from_slice(&(v as f32).to_le_bytes());
        }
    }
    buf
}

fn run(args: &Args) -> Result<(), String> {
    let champ_path = args.weights.as_ref().expect("--weights checked in parse_args");
    let weights = load_weights(champ_path)
        .map_err(|e| format!("loading champion weights from {}: {e}", champ_path.display()))?;
    let dims = WeightKey::ALL.len();

    let mut keys_name = args.out.clone().into_os_string();
    keys_name.push(".keys");
    let keys_path = PathBuf::from(keys_name);
    let names: Vec<String> = WeightKey::ALL.iter().map(|k| format!("{k:?}")).collect();
    std::fs::write(&keys_path, names.join("\n") + "\n")
        .map_err(|e| format!("writing {}: {e}", keys_path.display()))?;

    let file = File::create(&args.out).map_err(|e| format!("creating {}: {e}", args.out.display()))?;
    let mut w = BufWriter::new(file);
    w.write_all(b"TPHI").map_err(|e| e.to_string())?;
    w.write_all(&1u32.to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(&(dims as u32).to_le_bytes()).map_err(|e| e.to_string())?;
    w.write_all(&0u32.to_le_bytes()).map_err(|e| e.to_string())?;

    let out = Mutex::new(w);
    let next = AtomicUsize::new(0);
    let records = AtomicUsize::new(0);
    let started = Instant::now();

    std::thread::scope(|scope| {
        for _ in 0..args.threads {
            scope.spawn(|| loop {
                let g = next.fetch_add(1, Ordering::Relaxed);
                if g >= args.games {
                    return;
                }
                let seed = args.seed + g as u64;
                let (rows, margins, shares) = play_and_collect(args.players, seed, weights);
                let buf = encode_game(game_id(args.players, seed), args.players, &rows, &margins, &shares);
                records.fetch_add(rows.len(), Ordering::Relaxed);
                out.lock().expect("dump writer mutex poisoned").write_all(&buf).expect("writing dump records");
            });
        }
    });

    out.lock().expect("dump writer mutex poisoned").flush().map_err(|e| e.to_string())?;
    let n = records.load(Ordering::Relaxed);
    let secs = started.elapsed().as_secs_f64();
    println!("games      {}", args.games);
    println!("players    {}", args.players);
    println!("weights    {}", champ_path.display());
    println!("dims       {dims}");
    println!("records    {n}");
    println!("out        {}", args.out.display());
    println!("keys       {}", keys_path.display());
    println!("elapsed    {secs:.1}s  ({:.1} games/s)", args.games as f64 / secs);
    Ok(())
}

fn main() {
    match parse_args() {
        Ok(None) => {}
        Ok(Some(args)) => {
            if let Err(e) = run(&args) {
                eprintln!("phidump: {e}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("phidump: {e}");
            std::process::exit(2);
        }
    }
}
