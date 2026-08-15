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
//!
//! # `--policy-checkpoint`/`--a-policy`/`--b-policy`: the policy head as a
//! move-ordering prior, one side at a time
//!
//! [`tta::bots::neural::policy_order::PolicyOrder`] permutes `plan::pick`'s
//! beam candidates at every node, most-preferred-by-the-net first, so a node
//! budget that starves before every legal move is examined sees the good
//! ones sooner (see that module's own top doc comment) -- ordering only,
//! nothing pruned. `--policy-checkpoint PATH` loads a `TTAPOL01` checkpoint
//! ONCE, here, at startup; `--a-policy`/`--b-policy` (only legal when the
//! matching side's kind is `plan`) turn the prior on for that side's `Bot::
//! Plan` seat, built by hand with [`Bot::plan_with_policy`] -- see that
//! constructor's own doc comment for why it is a separate `Bot` variant
//! rather than a field on `Bot::Plan` itself. Neither flag changes the OFF
//! side at all: an untouched `Bot::Plan` from `build_bots`, byte-for-byte
//! what every other caller of this binary already gets.
//!
//! ```text
//! kindmatch --a plan --b plan --a-policy \
//!     --policy-checkpoint control.ckpt --weights champ.json \
//!     --games 240 --players 2 --threads 2
//! ```
//!
//! # `--max-nodes`: both sides, always
//!
//! `plan::PlanConfig::max_nodes` (default 4000) is a search-shape knob a
//! `plan`/`human+search`/policy-ordered seat all read; a move-ordering prior
//! can only change which candidates a search reaches when the budget it
//! reorders candidates for actually runs out before the tree does (see
//! `docs/NEURAL.md`'s policy-head follow-up). `--max-nodes N` overrides it
//! for EVERY seat this binary builds -- `--a`/`--b` alike, whichever kinds
//! they are -- never just one side: this is a controlled comparison of move
//! ORDER at a fixed budget, not a handicap match with two different
//! budgets. Omitting the flag leaves every seat at `PlanConfig::default()`'s
//! own 4000, byte-for-byte what every invocation before this flag existed
//! got.
//!
//! ```text
//! kindmatch --a plan --b plan --a-policy \
//!     --policy-checkpoint control.ckpt --weights champ.json \
//!     --max-nodes 400 --games 240 --players 2 --threads 2
//! ```

use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tta::bots::greedy::{build_bots, Bot, BotKind, Search, Seat};
use tta::bots::neural::net::ValueNet;
use tta::bots::neural::policy_order::PolicyOrder;
use tta::bots::neural::policy_train::load_policy_checkpoint;
use tta::bots::plan;
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
    /// See this file's module doc comment,
    /// "`--policy-checkpoint`/`--a-policy`/`--b-policy`". Only legal when
    /// the matching side's kind is [`BotKind::Plan`] and `--policy-
    /// checkpoint` is also given ([`parse_args`] rejects every other
    /// combination, including the checkpoint being given but neither flag
    /// naming a side to use it).
    policy_checkpoint: Option<String>,
    a_policy: bool,
    b_policy: bool,
    /// Deliberately worst-first ordering on the `--a-policy` seat --
    /// [`PolicyOrder::set_invert`]'s own doc comment. Only legal alongside
    /// `--a-policy` ([`parse_args`] rejects it otherwise); there is no
    /// `--b-policy-invert` because this flag exists for one falsification
    /// probe (this file's module doc comment,
    /// "`--policy-checkpoint`/`--a-policy`/`--b-policy`"), not as a general
    /// per-side feature.
    a_policy_invert: bool,
    /// See this file's module doc comment, "`--max-nodes`": applied to
    /// EVERY seat, never just one side.
    max_nodes: i64,
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
            policy_checkpoint: None,
            a_policy: false,
            b_policy: false,
            a_policy_invert: false,
            max_nodes: plan::PlanConfig::default().max_nodes,
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
  --policy-checkpoint PATH  a TTAPOL01 policy checkpoint, loaded once
  --a-policy        order --a's beam with the policy prior (needs --a plan
                     and --policy-checkpoint)
  --b-policy        order --b's beam with the policy prior (needs --b plan
                     and --policy-checkpoint)
  --a-policy-invert order --a's beam WORST-first instead (needs --a-policy;
                     a falsification probe, see PolicyOrder::set_invert)
  --max-nodes N   plan::PlanConfig::max_nodes for EVERY seat, both sides
                     (default 4000)
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

/// `--max-nodes`'s one write site: every [`Bot`] variant that carries a
/// [`plan::PlanConfig`] gets `max_nodes` set on it, whatever kind it ended
/// up as (`plan`, `human+search`, or policy-ordered `plan`) -- an exhaustive
/// match with no wildcard arm, so a future `Bot` variant that adds its own
/// `PlanConfig` is a compile error here instead of a silent miss. Every
/// other kind has no node budget to set and is left untouched.
fn set_max_nodes(bot: &mut Bot, max_nodes: i64) {
    match bot {
        Bot::Plan { cfg, .. } | Bot::HumanPlan { cfg, .. } | Bot::PlanWithPolicy { cfg, .. } => {
            cfg.max_nodes = max_nodes;
        }
        Bot::Random(_)
        | Bot::Greedy(_)
        | Bot::Weighted(_)
        | Bot::Quiescent { .. }
        | Bot::Book(_)
        | Bot::Variant(_)
        | Bot::Human(_) => {}
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
            "--policy-checkpoint" => a.policy_checkpoint = Some(value(flag)?),
            "--a-policy" => a.a_policy = true,
            "--b-policy" => a.b_policy = true,
            "--a-policy-invert" => a.a_policy_invert = true,
            "--max-nodes" => a.max_nodes = parse_num(&value(flag)?, flag)?,
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
    if a.a_policy && a.a != BotKind::Plan {
        return Err("--a-policy is only legal when --a is plan".to_string());
    }
    if a.b_policy && a.b != BotKind::Plan {
        return Err("--b-policy is only legal when --b is plan".to_string());
    }
    if (a.a_policy || a.b_policy) && a.policy_checkpoint.is_none() {
        return Err("--a-policy/--b-policy need --policy-checkpoint".to_string());
    }
    if a.policy_checkpoint.is_some() && !a.a_policy && !a.b_policy {
        return Err("--policy-checkpoint given but neither --a-policy nor --b-policy was set".to_string());
    }
    if a.a_policy_invert && !a.a_policy {
        return Err("--a-policy-invert is only legal alongside --a-policy".to_string());
    }
    if a.max_nodes <= 0 {
        return Err(format!("--max-nodes must be positive, got {}", a.max_nodes));
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
/// `policy_net`: the checkpoint `main` loaded ONCE, shared read-only across
/// every worker thread and every game as an `Arc` -- see [`Bot::
/// plan_with_policy`]'s own doc comment for why each policy-on seat below
/// still gets its OWN `PolicyOrder` (a cheap in-memory clone of the net
/// inside it, not a re-read of `PATH`) rather than sharing one mutable
/// scratch space across threads.
fn play_one(args: &Args, index: usize, policy_net: Option<&Arc<ValueNet>>) -> f64 {
    let players = args.players as usize;
    let seat = index % players;
    let seed = (args.seed.wrapping_add((index / players) as u64))
        .wrapping_mul(7919)
        .wrapping_add(17);

    // `--a-search`/`--b-search`: give the flagged side `Search::
    // HumanShortlistBeam` right in its `Seat`, so `build_bots` below
    // constructs `Bot::human_plan` itself -- no more hand-built overwrite
    // after the fact. `eval_weights` is the OTHER side's own weight vector,
    // since that is the one real gameplay evaluator already loaded for this
    // match (this file's module doc comment, "`--a-search`/`--b-search`").
    let seats: Vec<Seat> = (0..players)
        .map(|i| {
            let is_a = i == seat;
            let (kind, weights) =
                if is_a { (args.a, args.a_weights) } else { (args.b, args.b_weights) };
            let search_on = if is_a { args.a_search } else { args.b_search };
            let search = if search_on {
                let eval_weights = if is_a { args.b_weights } else { args.a_weights };
                Search::HumanShortlistBeam { eval_weights }
            } else {
                Search::None
            };
            Seat { kind, weights, search }
        })
        .collect();
    let mut bots = build_bots(&seats, seed as i64);
    // `--a-policy`/`--b-policy`: swap the flagged seat's plain `Bot::Plan`
    // for `Bot::plan_with_policy` -- same `--a-search`/`--b-search` shape
    // just above (overwrite after the unconditional `build_bots`), but
    // `--a`/`--b` kind is `plan` on both sides already (`parse_args`
    // enforces it), so only the move ORDER differs from the untouched side.
    for (i, s) in seats.iter().enumerate() {
        let policy_on = if i == seat { args.a_policy } else { args.b_policy };
        if !policy_on {
            continue;
        }
        let net = policy_net
            .expect("parse_args guarantees --policy-checkpoint whenever --a-policy/--b-policy is set");
        let player_seed = (seed as i64).wrapping_mul(131).wrapping_add(i as i64);
        let mut order = PolicyOrder::from_net((**net).clone());
        // `--a-policy-invert`: only ever set on the `--a` seat (`parse_args`
        // ties it to `--a-policy`), so a `--b-policy` seat never sees it --
        // `i == seat` is exactly "this is the `--a` side's seat this game".
        if i == seat && args.a_policy_invert {
            order.set_invert(true);
        }
        bots[i] = Bot::plan_with_policy(s.weights, order, player_seed);
    }
    // `--max-nodes`: every seat, unconditionally, whichever kind it ended up
    // as above -- see this file's module doc comment, "`--max-nodes`", for
    // why both sides must move together.
    for bot in bots.iter_mut() {
        set_max_nodes(bot, args.max_nodes);
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

    // Loaded ONCE, here, before any game or thread starts -- this module's
    // "`--policy-checkpoint`" doc comment. `Arc` so every worker thread
    // shares this one already-parsed net read-only; nobody mutates it
    // (`PolicyOrder::from_net` clones it per policy-on seat instead of
    // borrowing it mutably, so the checkpoint file itself is read exactly
    // once no matter how many threads or games follow).
    let policy_net: Option<Arc<ValueNet>> = match &args.policy_checkpoint {
        Some(path) => match load_policy_checkpoint(std::path::Path::new(path)) {
            Ok((net, _meta)) => Some(Arc::new(net)),
            Err(e) => {
                eprintln!("kindmatch: --policy-checkpoint: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };

    let started = Instant::now();
    let next = AtomicUsize::new(0);
    let done: Vec<Vec<(usize, f64)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..args.threads)
            .map(|_| {
                let (next, args, policy_net) = (&next, &args, &policy_net);
                scope.spawn(move || {
                    let mut mine = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        if index >= args.games {
                            return mine;
                        }
                        mine.push((index, play_one(args, index, policy_net.as_ref())));
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

    /// `--policy-checkpoint`/`--a-policy` parse into the exact `Args` fields
    /// `play_one`'s policy-overwrite loop reads -- `parse_args` never opens
    /// the checkpoint path itself (only `main`/`play_one` do), so this test
    /// needs no real checkpoint file on disk, just a path string.
    #[test]
    fn policy_checkpoint_and_a_policy_parse_into_the_matching_args_fields() {
        let argv = vec![
            "--a".to_string(),
            "plan".to_string(),
            "--b".to_string(),
            "plan".to_string(),
            "--policy-checkpoint".to_string(),
            "control.ckpt".to_string(),
            "--a-policy".to_string(),
        ];
        let args = parse_args(&argv).unwrap().unwrap();
        assert_eq!(args.policy_checkpoint.as_deref(), Some("control.ckpt"));
        assert!(args.a_policy, "--a-policy should have set a_policy");
        assert!(!args.b_policy, "--b-policy was never passed, so b_policy must stay off");
    }

    /// With none of `--policy-checkpoint`/`--a-policy`/`--b-policy` on the
    /// command line, every field `play_one`'s policy-overwrite loop gates on
    /// is at its off default -- so that loop's `if !policy_on { continue }`
    /// fires for every seat and every seat stays the plain `Bot::Plan`
    /// `build_bots` already built: the prior is off unless a caller opts
    /// in, exactly like [`pick_collecting`](tta::bots::plan::pick_collecting)'s
    /// own `policy: Option<&mut PolicyOrder>` parameter being `None` by
    /// default for every OTHER caller in this crate.
    #[test]
    fn omitting_every_policy_flag_leaves_the_prior_off_by_default() {
        let args =
            parse_args(&["--a".to_string(), "plan".to_string(), "--b".to_string(), "plan".to_string()])
                .unwrap()
                .unwrap();
        assert_eq!(args.policy_checkpoint, None);
        assert!(!args.a_policy);
        assert!(!args.b_policy);
    }

    /// `--a-policy` only makes sense on a `plan` seat -- `Bot::plan_with_
    /// policy` builds a `Bot::Plan`-shaped search, so flagging a `weighted`
    /// or `human` side for it would silently do nothing useful; rejected at
    /// parse time instead, the same way `--a-search` is rejected for a
    /// non-human `--a`.
    #[test]
    fn a_policy_is_rejected_when_a_is_not_plan() {
        let argv = vec![
            "--a".to_string(),
            "weighted".to_string(),
            "--a-policy".to_string(),
            "--policy-checkpoint".to_string(),
            "control.ckpt".to_string(),
        ];
        assert!(parse_args(&argv).is_err());
    }

    /// `--a-policy` with no `--policy-checkpoint` names a side to order but
    /// gives it nothing to order WITH -- rejected at parse time rather than
    /// panicking later in `play_one`'s `.expect(...)`.
    #[test]
    fn a_policy_without_a_policy_checkpoint_is_rejected() {
        let argv = vec!["--a".to_string(), "plan".to_string(), "--a-policy".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    /// `--policy-checkpoint` with neither `--a-policy` nor `--b-policy` set
    /// loads a checkpoint nothing will ever use -- rejected at parse time as
    /// a likely-typo'd invocation, rather than silently running the OFF
    /// path while the caller believes the prior is on.
    #[test]
    fn policy_checkpoint_given_with_no_side_enabled_is_rejected() {
        let argv = vec!["--policy-checkpoint".to_string(), "control.ckpt".to_string()];
        assert!(parse_args(&argv).is_err());
    }

    // -------------------------------------------------------- --max-nodes

    /// Omitting `--max-nodes` must leave every seat at `plan::PlanConfig`'s
    /// OWN default (4000 today), read from the real constant rather than a
    /// copied literal -- every invocation of this binary before this flag
    /// existed gets byte-for-byte the same search budget it always did.
    #[test]
    fn omitting_max_nodes_leaves_the_planconfig_default_unchanged() {
        let args =
            parse_args(&["--a".to_string(), "plan".to_string(), "--b".to_string(), "plan".to_string()])
                .unwrap()
                .unwrap();
        assert_eq!(args.max_nodes, plan::PlanConfig::default().max_nodes);
    }

    #[test]
    fn max_nodes_flag_parses_into_the_matching_args_field() {
        let argv = vec!["--max-nodes".to_string(), "400".to_string()];
        let args = parse_args(&argv).unwrap().unwrap();
        assert_eq!(args.max_nodes, 400);
    }

    #[test]
    fn max_nodes_rejects_a_nonpositive_value() {
        let argv = vec!["--max-nodes".to_string(), "0".to_string()];
        assert!(parse_args(&argv).is_err(), "a zero node budget can never run a search");
    }

    /// [`set_max_nodes`] reaches every `plan`-shaped `Bot` variant and
    /// leaves every other kind untouched -- the direct proof that
    /// `--max-nodes` is not silently a no-op for whichever `Bot`
    /// construction path a seat took (plain `build_bots`, `Bot::human_plan`,
    /// or `Bot::plan_with_policy`).
    #[test]
    fn set_max_nodes_overrides_every_plan_shaped_bot_and_ignores_everything_else() {
        let mut plan_bot = Bot::Plan {
            cfg: plan::PlanConfig::default(),
            stats: plan::Stats::default(),
            counters: tta::bots::pending::Counters::default(),
            rng: tta::rng::PyRandom::new(1),
        };
        set_max_nodes(&mut plan_bot, 400);
        match plan_bot {
            Bot::Plan { cfg, .. } => assert_eq!(cfg.max_nodes, 400),
            Bot::Random(_) | Bot::Greedy(_) | Bot::Weighted(_) | Bot::Quiescent { .. } | Bot::Book(_) | Bot::Variant(_) | Bot::Human(_) | Bot::HumanPlan { .. } | Bot::PlanWithPolicy { .. } => panic!("kind must not change"),
        }

        let mut random_bot = Bot::Random(tta::bots::greedy::RandomBot::new(1));
        set_max_nodes(&mut random_bot, 400);
        assert_eq!(random_bot.kind(), BotKind::Random, "a kind with no PlanConfig must be untouched");
    }
}
