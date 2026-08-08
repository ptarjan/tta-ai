//! `kindmatch` -- duel two BOT KINDS (not two weight vectors) at the same
//! table, on shared deals, by default playing the SAME weights on both
//! sides (`--weights`), or two DIFFERENT weight files (`--a-weights`/
//! `--b-weights`) when the two kinds are not fit to the same vocabulary.
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
//!
//! # `--a-weights`/`--b-weights`: for when the two kinds cannot share a file
//!
//! [`BotKind::Human`] is fit to imitate human move CHOICES
//! (`bots::human`'s doc comment), never `dominance_repair`-ed the way a
//! champion vector is, and is loaded with `human_policy::load_weights`
//! instead of `bots::weighted::eval::load_weights` for exactly that reason
//! -- so a `human` side sharing `--weights` with a `weighted` champion side
//! is never correct: either the human file gets `dominance_repair` applied
//! (an invariant it was never fit to satisfy), or the champion side plays
//! under a vector it wasn't trained under. `--a-weights PATH`/`--b-weights
//! PATH` give each side its own file, each loaded with the loader its OWN
//! `--a`/`--b` kind calls for (see [`loader_for`]); `--weights` remains the
//! shared fallback for whichever side has no per-side override, unchanged
//! from before this existed so every prior invocation is byte-for-byte
//! unaffected.
//!
//! ```text
//! kindmatch --a human --b weighted \
//!     --a-weights human_weights.json --b-weights champion_3p.json \
//!     --games 480 --players 3 --threads 6
//! ```
//!
//! # `--a-search`/`--b-search`: real lookahead for a `human` side
//!
//! Plain `--a human` is [`tta::bots::human::HumanBot`]: a pure argmax over
//! `human_policy::predict_top1`, no search at all -- see that module's own
//! doc comment. `--a-search` (only legal when `--a human`) swaps that side
//! for [`tta::bots::greedy::Bot::human_plan`] instead: `human_policy`'s
//! ranking model narrows the root to a shortlist of human-plausible moves,
//! then [`tta::bots::plan::pick`]'s ordinary beam -- scored by the OTHER
//! side's own weight vector, since that is the one real gameplay evaluator
//! already loaded for this match and `Seat` has no second vector slot to
//! carry a dedicated one -- picks among what survives. This is a HYBRID,
//! not a stronger human imitator; see `bots::human::choose_with_search`'s
//! own doc comment for the honesty note. Plain `--a human` (no `--search`)
//! is completely untouched by this flag, so `humanpaired`'s imitation-
//! accuracy numbers never change meaning.
//!
//! ```text
//! kindmatch --a human --a-search --b weighted \
//!     --a-weights human_weights.json --b-weights champion_3p.json \
//!     --games 480 --players 3 --threads 6
//! ```

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, Bot, BotKind, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::game::{self, MOVE_CAP};
use tta::human_policy;
use tta::stats;

#[derive(Clone, Debug)]
struct Args {
    a: BotKind,
    b: BotKind,
    /// Resolved AFTER the whole command line is parsed (see
    /// [`parse_args`]'s tail) -- which loader is correct for each side
    /// depends on that side's FINAL kind, which may be typed after
    /// `--weights`/`--a-weights`/`--b-weights` on the command line.
    a_weights: Weights,
    b_weights: Weights,
    weights_path: Option<String>,
    a_weights_path: Option<String>,
    b_weights_path: Option<String>,
    /// See this file's module doc comment, "`--a-search`/`--b-search`".
    /// Only legal when the matching side's kind is [`BotKind::Human`]
    /// ([`parse_args`] rejects any other combination).
    a_search: bool,
    b_search: bool,
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
            a_weights: Weights::defaults(),
            b_weights: Weights::defaults(),
            weights_path: None,
            a_weights_path: None,
            b_weights_path: None,
            a_search: false,
            b_search: false,
            games: 60,
            players: 3,
            seed: 0,
            threads: 1,
        }
    }
}

const USAGE: &str = "\
usage: kindmatch --a KIND --b KIND [options]

  --a KIND          challenger bot kind, seated one per game
  --b KIND          defender bot kind, seated in every other chair
  --weights PATH    weight vector BOTH sides play, absent a per-side
                     override below (default: built-in defaults)
  --a-weights PATH  weight file for the --a side only (default: --weights)
  --b-weights PATH  weight file for the --b side only (default: --weights)
  --a-search        give --a real lookahead (only legal with --a human)
  --b-search        give --b real lookahead (only legal with --b human)
  --games N       games; rounded down to a whole number of deals (default 60)
  --players N     2, 3 or 4 (default 3)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
  --help
";

/// The loader a kind's weight file must go through -- `BotKind::Human` is
/// fit to imitate choices, never `dominance_repair`-ed, and so reads with
/// [`human_policy::load_weights`] instead of the champion loader every other
/// evaluator kind uses; see this file's module doc comment. Both loaders
/// parse the same JSON shape (`Weights`), so a caller who passes the wrong
/// file for a kind gets a wrong-but-valid vector, not a parse error --
/// exactly the silent mismatch naming each side's loader by ITS OWN kind
/// exists to avoid.
fn loader_for(kind: BotKind) -> fn(&std::path::Path) -> Result<Weights, String> {
    match kind {
        BotKind::Human => human_policy::load_weights,
        BotKind::Random
        | BotKind::Greedy
        | BotKind::Weighted
        | BotKind::Quiescent
        | BotKind::Plan
        | BotKind::Book
        | BotKind::Culture
        | BotKind::Military
        | BotKind::Science
        | BotKind::Wonder
        | BotKind::Infra
        | BotKind::Tempo => load_weights,
    }
}

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
            "--weights" => a.weights_path = Some(value(flag)?),
            "--a-weights" => a.a_weights_path = Some(value(flag)?),
            "--b-weights" => a.b_weights_path = Some(value(flag)?),
            "--a-search" => a.a_search = true,
            "--b-search" => a.b_search = true,
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
    if a.a_search && a.a != BotKind::Human {
        return Err("--a-search is only legal when --a is human".to_string());
    }
    if a.b_search && a.b != BotKind::Human {
        return Err("--b-search is only legal when --b is human".to_string());
    }
    let per_deal = a.players as usize;
    a.games -= a.games % per_deal;
    if a.games == 0 {
        return Err(format!("--games must be at least {per_deal} at {}p", a.players));
    }
    // Resolve each side's vector now that both kinds are final: a per-side
    // path wins, else fall back to the shared `--weights`, else the
    // built-in default this struct already started from.
    if let Some(path) = a.a_weights_path.clone().or_else(|| a.weights_path.clone()) {
        a.a_weights = loader_for(a.a)(std::path::Path::new(&path))?;
    }
    if let Some(path) = a.b_weights_path.clone().or_else(|| a.weights_path.clone()) {
        a.b_weights = loader_for(a.b)(std::path::Path::new(&path))?;
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
        .map(|i| {
            if i == seat {
                Seat { kind: args.a, weights: args.a_weights }
            } else {
                Seat { kind: args.b, weights: args.b_weights }
            }
        })
        .collect();
    let mut bots = build_bots(&seats, seed as i64);
    // `--a-search`/`--b-search`: swap the flagged seat's plain, no-lookahead
    // `Bot::Human` for `Bot::human_plan` -- built by hand, not through
    // `build_bots`, since that variant needs a SECOND vector `Seat` has no
    // slot for (this file's module doc comment, "`--a-search`/`--b-search`").
    // `build_bots` above is still called first and unconditionally, so every
    // OTHER seat's construction and seeding is byte-for-byte what it always
    // was; only the flagged index is overwritten afterward.
    for (i, s) in seats.iter().enumerate() {
        let search = if i == seat { args.a_search } else { args.b_search };
        if !search {
            continue;
        }
        // The evaluator `plan::pick`'s beam scores every node with: the
        // OTHER side's own weight vector, since that is the one real
        // gameplay evaluator already loaded for this match (see this file's
        // module doc comment).
        let eval_weights = if i == seat { args.b_weights } else { args.a_weights };
        let human_weights = human_policy::vector_from_weights(&s.weights);
        let player_seed = (seed as i64).wrapping_mul(131).wrapping_add(i as i64);
        bots[i] = Bot::human_plan(eval_weights, human_weights, player_seed);
    }

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
    println!("A (rotates)  {}{}", args.a.name(), if args.a_search { "+search" } else { "" });
    println!("B (rest)     {}{}", args.b.name(), if args.b_search { "+search" } else { "" });
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

#[cfg(test)]
mod tests {
    use super::*;
    use tta::bots::weighted::eval::save_weights;
    use tta::bots::weighted::weights::WeightKey;

    /// [`loader_for`] is a total, exhaustive match with no wildcard arm, so
    /// every [`BotKind`] this crate defines must resolve to exactly one of
    /// the two real loaders -- this test pins that `Human` alone gets
    /// [`human_policy::load_weights`] and every other kind gets the champion
    /// [`load_weights`], by comparing function-pointer identity rather than
    /// behaviour, so it does not need a file on disk.
    #[test]
    fn loader_for_resolves_the_human_kind_to_the_human_policy_loader_and_every_other_kind_to_the_champion_loader() {
        for &kind in BotKind::ALL {
            let got = loader_for(kind) as *const ();
            let want = if kind == BotKind::Human {
                human_policy::load_weights as *const ()
            } else {
                load_weights as *const ()
            };
            assert_eq!(got, want, "{kind:?} resolved to the wrong loader");
        }
    }

    /// With no `--a-weights`/`--b-weights` override, both sides fall back to
    /// the shared `--weights` file -- the exact behaviour every invocation
    /// of `kindmatch` had before per-side overrides existed, so this pins
    /// that adding them did not change the old default path.
    #[test]
    fn parse_args_gives_both_sides_the_shared_weights_flag_when_no_per_side_override_is_given() {
        let dir = std::env::temp_dir().join(format!("kindmatch_test_shared_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("shared.json");
        let mut w = Weights::defaults();
        w.set(WeightKey::ResourceStock, 3.5);
        load_weights_via_champion_save(&path, &w);

        let argv = vec!["--weights".to_string(), path.display().to_string()];
        let args = parse_args(&argv).unwrap().unwrap();
        assert_eq!(args.a_weights.get(WeightKey::ResourceStock), 3.5);
        assert_eq!(args.b_weights.get(WeightKey::ResourceStock), 3.5);
        std::fs::remove_file(&path).ok();
    }

    /// A `--b human` side reads its weight file with the human-imitation
    /// loader (no `dominance_repair`) even while `--a weighted` reads the
    /// SAME shared `--weights` file with the champion loader (which DOES
    /// repair it) -- proving `parse_args` picks the loader by each side's
    /// OWN final kind, not one loader for the whole invocation. The fixture
    /// sets `BlueFree` above `ResourceStock`, a real rule `DOMINATES`
    /// requires the other way around, so the champion side's `ResourceStock`
    /// must come back repaired upward while the human side's must not.
    #[test]
    fn a_human_side_skips_dominance_repair_while_a_weighted_side_sharing_the_same_file_gets_it() {
        let dir = std::env::temp_dir().join(format!("kindmatch_test_mixed_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("violating.json");
        let mut w = Weights::defaults();
        w.set(WeightKey::ResourceStock, 0.0);
        w.set(WeightKey::BlueFree, 10.0);
        human_policy::save_weights(&path, &w).unwrap();

        let argv = vec![
            "--a".to_string(),
            "weighted".to_string(),
            "--b".to_string(),
            "human".to_string(),
            "--weights".to_string(),
            path.display().to_string(),
        ];
        let args = parse_args(&argv).unwrap().unwrap();
        assert!(
            args.a_weights.get(WeightKey::ResourceStock) >= 10.0,
            "weighted side should have been dominance-repaired upward, got {}",
            args.a_weights.get(WeightKey::ResourceStock)
        );
        assert_eq!(
            args.b_weights.get(WeightKey::ResourceStock),
            0.0,
            "human side should NOT have been dominance-repaired"
        );
        std::fs::remove_file(&path).ok();
    }

    /// `--a-weights`/`--b-weights` override the shared `--weights` file for
    /// their own side only -- the whole point of adding them (see this
    /// file's module doc comment): a `human` side and a `weighted` side
    /// almost never want to share ONE fitted vector.
    #[test]
    fn per_side_weights_flags_override_the_shared_weights_flag_for_that_side_only() {
        let dir = std::env::temp_dir().join(format!("kindmatch_test_override_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shared = dir.join("shared2.json");
        let a_only = dir.join("a_only.json");
        let mut w_shared = Weights::defaults();
        w_shared.set(WeightKey::ResourceStock, 1.0);
        load_weights_via_champion_save(&shared, &w_shared);
        let mut w_a = Weights::defaults();
        w_a.set(WeightKey::ResourceStock, 9.0);
        load_weights_via_champion_save(&a_only, &w_a);

        let argv = vec![
            "--weights".to_string(),
            shared.display().to_string(),
            "--a-weights".to_string(),
            a_only.display().to_string(),
        ];
        let args = parse_args(&argv).unwrap().unwrap();
        assert_eq!(args.a_weights.get(WeightKey::ResourceStock), 9.0);
        assert_eq!(args.b_weights.get(WeightKey::ResourceStock), 1.0);
        std::fs::remove_file(&shared).ok();
        std::fs::remove_file(&a_only).ok();
    }

    /// Writes a flat weight file with the champion saver so the fixture is
    /// read back identically by whichever loader a test needs, matching how
    /// `analysis/frozen/*.json` champion files are actually laid out on
    /// disk (no `extra` bookkeeping fields, so it stays flat -- see
    /// `parse_weights`'s fallback-to-whole-doc branch).
    fn load_weights_via_champion_save(path: &std::path::Path, w: &Weights) {
        save_weights(path, w, &[]).unwrap();
    }
}
