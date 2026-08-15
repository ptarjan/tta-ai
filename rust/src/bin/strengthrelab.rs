//! `strengthrelab` -- earlymil task, step 3 measurement.
//!
//! Plays the champion's own evaluator, AS COMMITTED (with the `StrengthRel`
//! structural fix, `eval::evaluate`), head-to-head against a LEGACY
//! comparator that reproduces the pre-fix formula, seat-paired and rotated,
//! exactly like `arena.rs`'s own `Match`/`Summary` design (reused here for
//! the pairing math via [`tta::stats::paired`] -- not reimplemented).
//!
//! Why this is a separate binary instead of an `arena --a --b` run: the fix
//! is a CODE change (how `evaluate` prices `WeightKey::StrengthRel`), not a
//! WEIGHT VALUE change, so the two sides cannot be expressed as two JSON
//! files played through the one shared `evaluate`. `arena.rs`'s `Seat`/
//! `BotKind` machinery has no slot for "same kind, different formula", so
//! this binary supplies its own tiny seat-pick dispatch and reuses
//! `game::play_game` (the same driver `Match::play_one` calls) directly.
//!
//! ## What "legacy" reproduces, and what it does not
//!
//! `evaluate_legacy_strength_rel` patches exactly the one thing the fix
//! changed: `evaluate`'s own top-level flat+phase treatment of
//! `WeightKey::StrengthRel` reverts to the pre-fix `base + (1-L)*early +
//! L*late` formula (an always-on base, see `eval::evaluate`'s own doc
//! comment for the full derivation) via a closed-form "what the old formula
//! would have added instead" correction added on top of the real, shipped
//! `evaluate`'s total. That is exact for the BUILD-decision mechanism this
//! whole task is about -- `bin/dumpweights.rs`'s step-1 dump (EARLYMIL.txt)
//! showed the identity-aware gates (`HandPotential`, `WonderPotential`, ...)
//! contribute NOTHING to a Bronze-vs-Warriors build comparison, only
//! `evaluate`'s own linear+phase body does.
//!
//! It is NOT exact for decisions where `rivals::strength_marginal` is
//! called a SECOND time, nested inside a card-potential gate (pricing a
//! military card still in hand, a tactic's reachability, and similar) --
//! those nested calls go through the SHIPPED (fixed) `strength_marginal`
//! for both sides here, because that function is shared production code and
//! duplicating it (and every caller between it and `evaluate`) was judged
//! out of proportion to what this measurement needs. This makes the
//! "legacy" opponent here a slightly WEAKER reproduction of the true
//! pre-fix bot than the real pre-fix binary would have been (its nested
//! card pricing is already a little less strength-happy than the original),
//! which is a conservative approximation for this A/B, not an inflated one.
//!
//! ```text
//! cargo run --profile difftest --bin strengthrelab -- \
//!     --games 3000 --players 2 --weights /private/tmp/rowdig/frozen_champion_2p.json --threads 2
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::apply;
use tta::bots::weighted::eval::{evaluate, load_weights, WeightedBot};
use tta::bots::weighted::horizon;
use tta::bots::weighted::rivals::{self, RivalContext};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::game::{self, MOVE_CAP};
use tta::moves::Move;
use tta::stats::{self, Estimate};

/// The closed-form "what the OLD `StrengthRel` formula would have added, in
/// place of what the shipped one just did" correction -- see this module's
/// top doc comment. Both formulas are linear in the same raw feature `v`,
/// so their difference is a single extra term, not a second walk over
/// `WeightKey::ALL`/`PHASE_KEYS`.
fn strength_rel_old_minus_new(state: &tta::state::GameState, idx: u8, w: &Weights, ctx: &RivalContext) -> f64 {
    // `evaluate` itself only uses the NEW formula while
    // `horizon::combat_unreachable` (STRGATE.txt; formerly a bare
    // `state.round <= 3`, see that function's own doc comment) -- once
    // combat is reachable it is ALREADY byte-identical to the old formula,
    // so there is nothing to correct there. Applying this correction
    // unconditionally would double-count for every post-opening decision
    // (`evaluate`'s real total is already "old"; adding `old - new` again
    // would make the legacy score `2*old - new`, not `old`).
    if !horizon::combat_unreachable(state) {
        return 0.0;
    }
    let f = tta::bots::weighted::features::features(state, idx, Some(ctx), Some(w), true);
    let v = f.get(WeightKey::StrengthRel);
    if v == 0.0 {
        return 0.0;
    }
    let late = horizon::lateness(state);
    let early = 1.0 - late;
    let old = w.get(WeightKey::StrengthRel) + early * w.get(WeightKey::StrengthRel.early()) + late * w.get(WeightKey::StrengthRel.late());
    let new = early * w.get(WeightKey::StrengthRel.early()) + late * (w.get(WeightKey::StrengthRel) + w.get(WeightKey::StrengthRel.late()));
    (old - new) * v
}

fn evaluate_legacy_strength_rel(state: &tta::state::GameState, idx: u8, w: &Weights, ctx: &RivalContext) -> f64 {
    evaluate(state, idx, w, Some(ctx), None) + strength_rel_old_minus_new(state, idx, w, ctx)
}

/// STRGATE.txt addition: the closed-form correction for the OTHER
/// comparison the task asks for -- shipped (`horizon::combat_unreachable`
/// gate) vs commit 578ee9e's own landed fix (a bare `state.round <= 3`
/// gate), NOT vs the pre-fix formula `strength_rel_old_minus_new` already
/// covers. Both sides compute the identical NEW phase-blended formula when
/// their own gate is open; they can only disagree in the (rare) states
/// where the two gates' booleans disagree, so the correction is zero
/// everywhere they agree and the same closed-form "new minus new" swap
/// (algebraically: old-vs-new correction with its sign/target flipped)
/// everywhere they don't.
fn strength_rel_roundgated_minus_shipped(state: &tta::state::GameState, idx: u8, w: &Weights, ctx: &RivalContext) -> f64 {
    let roundgated_open = state.round <= 3;
    let shipped_open = horizon::combat_unreachable(state);
    if roundgated_open == shipped_open {
        return 0.0; // both sides used the same branch; evaluate()'s real total already matches.
    }
    let f = tta::bots::weighted::features::features(state, idx, Some(ctx), Some(w), true);
    let v = f.get(WeightKey::StrengthRel);
    if v == 0.0 {
        return 0.0;
    }
    let late = horizon::lateness(state);
    let early = 1.0 - late;
    let old = w.get(WeightKey::StrengthRel) + early * w.get(WeightKey::StrengthRel.early()) + late * w.get(WeightKey::StrengthRel.late());
    let new = early * w.get(WeightKey::StrengthRel.early()) + late * (w.get(WeightKey::StrengthRel) + w.get(WeightKey::StrengthRel.late()));
    // `evaluate`'s real total (what `shipped_open` produced) is already one
    // of {old, new}; the correction is "what round-gated would have used"
    // minus "what shipped actually used".
    let roundgated_val = if roundgated_open { new } else { old };
    let shipped_val = if shipped_open { new } else { old };
    (roundgated_val - shipped_val) * v
}

fn evaluate_roundgated_strength_rel(state: &tta::state::GameState, idx: u8, w: &Weights, ctx: &RivalContext) -> f64 {
    evaluate(state, idx, w, Some(ctx), None) + strength_rel_roundgated_minus_shipped(state, idx, w, ctx)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opponent {
    /// The pre-fix, always-on-base formula (commit 6168d5a and earlier).
    Legacy,
    /// Commit 578ee9e's own landed fix: the new phase-blended formula
    /// gated on a bare `state.round <= 3` instead of
    /// `horizon::combat_unreachable`.
    RoundGated,
}

/// Mirrors `WeightedBot::choose` exactly (same 1-ply trial-and-argmax loop,
/// same `EndTurnBias` handling on `Move::EndTurn`), scoring every candidate
/// with the LEGACY formula instead of the shipped one.
fn choose_legacy(state: &tta::state::GameState, moves: &[Move], w: &Weights, opponent: Opponent) -> Move {
    // `bots::filter_resign` is `pub(crate)`, unreachable from a `bin`
    // crate -- inlined rather than widening that visibility for one
    // measurement tool. Same three-line shape `WeightedBot::choose`'s own
    // doc comment describes: drop `Move::Resign` unless it is the only
    // legal move.
    let filtered: Vec<Move> = if moves.iter().any(|m| !matches!(m, Move::Resign)) {
        moves.iter().copied().filter(|m| !matches!(m, Move::Resign)).collect()
    } else {
        moves.to_vec()
    };
    let moves: &[Move] = &filtered;
    if moves.len() == 1 {
        return moves[0];
    }
    let idx = state.decider();
    let ctx = rivals::rival_context(state, idx, None, None);
    let end_bias = w.get(WeightKey::EndTurnBias);

    let mut best: Option<(Move, f64)> = None;
    for &mv in moves {
        let mut trial = state.clone();
        apply::apply(&mut trial, mv);
        let mut val = match opponent {
            Opponent::Legacy => evaluate_legacy_strength_rel(&trial, idx, w, &ctx),
            Opponent::RoundGated => evaluate_roundgated_strength_rel(&trial, idx, w, &ctx),
        };
        if matches!(mv, Move::EndTurn) {
            val += end_bias;
        }
        if best.is_none_or(|(_, bv)| val > bv) {
            best = Some((mv, val));
        }
    }
    best.map(|(m, _)| m).unwrap_or(moves[0])
}

/// One paired game's result from the FIXED bot's point of view -- same
/// shape as `arena::Duel`, but this binary owns its own tiny copy rather
/// than depending on `arena`'s private fields.
struct Duel {
    share: f64,
    cap_hit: bool,
}

fn play_one(w: &Weights, players: u8, index: usize, seed0: u64, opponent: Opponent) -> Duel {
    let players_usize = players as usize;
    let fixed_seat = index % players_usize;
    let seed = (seed0.wrapping_add((index / players_usize) as u64)).wrapping_mul(7919).wrapping_add(17);

    let fixed_bot = WeightedBot::new(*w);
    let mut state = game::new_game(players, seed);
    let outcome = game::play_game(&mut state, MOVE_CAP, |s, legal_moves| {
        if s.current as usize == fixed_seat {
            fixed_bot.choose(s, legal_moves.as_slice())
        } else {
            choose_legacy(s, legal_moves.as_slice(), w, opponent)
        }
    });

    let winners = game::winners(&state);
    let share = if winners.contains(&(fixed_seat as u8)) { 1.0 / winners.len() as f64 } else { 0.0 };
    Duel { share, cap_hit: outcome.move_cap_hit }
}

fn play(w: &Weights, players: u8, games: usize, seed0: u64, threads: usize, opponent: Opponent) -> Vec<Duel> {
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<(usize, Duel)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                let next = &next;
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= games {
                            return mine;
                        }
                        mine.push((index, play_one(w, players, index, seed0, opponent)));
                    }
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("worker thread panicked")).collect()
    });
    let mut slots: Vec<Option<Duel>> = Vec::with_capacity(games);
    slots.resize_with(games, || None);
    for (index, duel) in done.into_iter().flatten() {
        slots[index] = Some(duel);
    }
    slots.into_iter().map(|d| d.expect("every index was played")).collect()
}

struct Args {
    games: usize,
    players: u8,
    weights_path: PathBuf,
    seed: u64,
    threads: usize,
    opponent: Opponent,
}

impl Default for Args {
    fn default() -> Args {
        Args { games: 1000, players: 2, weights_path: PathBuf::new(), seed: 0, threads: 1, opponent: Opponent::Legacy }
    }
}

const USAGE: &str = "\
usage: strengthrelab --weights PATH [options]

  --games N       games; rounded DOWN to a whole number of deals (default 1000)
  --players N     2, 3 or 4 (default 2)
  --weights PATH  champion JSON both sides play (required)
  --seed N        base deal seed (default 0)
  --threads N     games in parallel (default 1)
  --opponent K    legacy (pre-fix, default) or roundgated (commit 578ee9e's
                  own state.round<=3 gate, STRGATE.txt's comparison point)
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
            "--games" => a.games = value(flag)?.parse().map_err(|_| "bad --games".to_string())?,
            "--players" => a.players = value(flag)?.parse().map_err(|_| "bad --players".to_string())?,
            "--weights" => a.weights_path = PathBuf::from(value(flag)?),
            "--seed" => a.seed = value(flag)?.parse().map_err(|_| "bad --seed".to_string())?,
            "--threads" => a.threads = value(flag)?.parse().map_err(|_| "bad --threads".to_string())?,
            "--opponent" => {
                a.opponent = match value(flag)?.as_str() {
                    "legacy" => Opponent::Legacy,
                    "roundgated" => Opponent::RoundGated,
                    other => return Err(format!("bad --opponent {other} (want legacy or roundgated)")),
                }
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
    if a.weights_path.as_os_str().is_empty() {
        return Err("--weights is required".to_string());
    }
    if a.threads == 0 {
        a.threads = 1;
    }
    let per_deal = a.players as usize;
    a.games -= a.games % per_deal;
    if a.games == 0 {
        return Err(format!("games must be at least {per_deal} at {}p", a.players));
    }
    Ok(Some(a))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("strengthrelab: {e}");
            return ExitCode::FAILURE;
        }
    };

    let w = match load_weights(&args.weights_path) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("strengthrelab: failed to load {}: {e}", args.weights_path.display());
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    let duels = play(&w, args.players, args.games, args.seed, args.threads, args.opponent);
    let elapsed = started.elapsed().as_secs_f64();

    let shares: Vec<Option<f64>> = duels.iter().map(|d| Some(d.share)).collect();
    let win: Estimate = stats::paired(&shares, args.players as usize);
    let null = 1.0 / args.players as f64;
    let cap_hits = duels.iter().filter(|d| d.cap_hit).count();

    println!("games        {} ({} deals x {} seats)", duels.len(), win.n_deals, args.players);
    println!("players      {}", args.players);
    println!("weights      {}", args.weights_path.display());
    println!("A            fixed (StrengthRel structural fix, combat_unreachable gate)");
    println!(
        "B            {}",
        match args.opponent {
            Opponent::Legacy => "legacy (pre-fix always-on-base formula)",
            Opponent::RoundGated => "roundgated (commit 578ee9e, bare state.round<=3 gate)",
        }
    );
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", duels.len() as f64 / elapsed);
    if cap_hits > 0 {
        println!("WARNING      {cap_hits} game(s) hit the {MOVE_CAP}-move cap -- that is a bug");
    }
    println!();
    println!(
        "win rate     {:.1}%  +/- {:.1}   (null {:.1}%)   p = {:.4}",
        100.0 * win.mean,
        100.0 * win.half,
        100.0 * null,
        win.p_against(null),
    );
    println!(
        "             [naive +/- {:.1}, rho {:+.2}, deff {:.2}, {} deals]",
        100.0 * win.naive_half,
        win.rho,
        win.deff,
        win.n_deals,
    );
    println!();
    if win.lo() > null {
        println!("verdict      accept -- fixed beats B (interval clear of the null)");
    } else if win.hi() < null {
        println!("verdict      reject -- fixed is WORSE than B");
    } else {
        println!("verdict      inconclusive -- the interval still straddles the null");
    }

    ExitCode::SUCCESS
}
