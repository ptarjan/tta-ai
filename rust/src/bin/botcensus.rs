//! `botcensus` -- the bot-side twin of `corpuscensus.rs`. That binary counts
//! action classes over the 1,011-game BGO human corpus by classifying journal
//! TEXT; this one counts the same classes over BOT self-play by classifying
//! real engine STATE, per the project's standing rule to prefer structural
//! counting over text matching wherever the source is available structurally
//! (it is, here -- self-play never has to go through a journal at all).
//!
//! ```text
//! cargo run --release --bin botcensus -- \
//!     --games 300 --players 2 --weights experiments/rust_champion_2p.json
//! ```
//!
//! Prints one Markdown-ish table (one column, this player count) to stdout;
//! `docs/HUMAN_PLAY.md`'s bot-vs-human section is three of these runs (2p,
//! 3p, 4p) pasted in by hand next to `corpuscensus`'s human numbers, exactly
//! as that binary's own doc comment describes its own output being pasted in.
//!
//! # Method: read the engine's own state transitions, not its move list
//!
//! Most of [`BotClass`] falls straight out of [`Move`] ([`classify_move`]) --
//! `Move::Take` IS a take, `Move::War` IS a declared war, no inference
//! needed. Four classes do not, because the human-corpus definition they
//! have to match (`corpuscensus.rs`'s `ActionClass`) is not "a move the
//! player chose" at all:
//!
//! - **`PlayEvent`**: BGO logs "X plays event" when a previously-prepared
//!   event card RESOLVES, which happens automatically (`events::
//!   reveal_current_event`, called from inside `Move::PrepareEvent`'s own
//!   handler) -- there is no `Move` for "an event fires". Detected here by
//!   diffing `state.past_events.len()` around every move (`events.rs::
//!   reveal_current_event` is that field's only writer for a non-territory
//!   card).
//! - **`WinAuction`** / **`Colonize`**: `WinAuction` is detected from the
//!   pending-decision stack (`Pending::Auction` present before a move, gone
//!   after, with a recorded high bidder) -- every auction requires at least
//!   one real `Bid`/`BidPass` from a bot to resolve, so it always surfaces
//!   as a separate decision this loop observes directly. `Colonize` is NOT
//!   independently detected by diffing `PlayerState::colonies` -- that field
//!   has a SECOND writer this file's first draft missed: `ChoiceKind::Annex`
//!   (`interact.rs`, a war/aggression spoil that steals an EXISTING colony
//!   from a rival) pushes onto the winner's `colonies` exactly the way a
//!   fresh colonization does, so a raw length diff counts an annexation as a
//!   colonize -- caught by the 2p run's colonize/win-auction ratio coming
//!   out 6x instead of near-1x, the same kind of suspicious-equality check
//!   `corpuscensus.rs`'s own "most surprising number" section used to catch
//!   its own bug the same way. Fixed by leaning on the corpus's own finding
//!   instead of re-deriving it: `interact::colonize`'s `assert!` (`"must
//!   colonize ... unreachable from an auction"`) guarantees a won auction
//!   ALWAYS completes into exactly one colonize (`interact::colonize_auto`
//!   runs any single-legal-continuation steps to completion synchronously,
//!   and multi-choice steps have no legal alternative to NOT finish
//!   colonizing), so `Colonize` is credited in lockstep with `WinAuction`
//!   rather than detected separately. `Move::ColumbusColonize` (§11.5, no
//!   auction at all) still credits `Colonize` on its own, with no paired
//!   `WinAuction` -- the one case where the two can legitimately diverge,
//!   flagged in `docs/HUMAN_PLAY.md`.
//! - **`WinWar`**: `combat::resolve_war_outcome` runs inside `start_turn`
//!   (called for whichever player's turn is beginning, itself a consequence
//!   of some earlier move, usually `Move::EndTurn`), and returns its winner
//!   to a caller this binary cannot see. Detected instead by watching
//!   `PlayerState::war_declared_by_me` clear (that function's one and only
//!   way to clear it) and, when it does, comparing the two sides' strength
//!   AS COMPUTED FROM THE PRE-MOVE STATE via the same public
//!   `effects::state_stats` the engine's own resolution reads -- the higher
//!   side is the victor, a tie counts neither way, matching
//!   `resolve_war_outcome`'s own tie handling exactly.
//!
//! `AcceptPact` and `Discard` are a lighter version of the same idea: both
//! are answers to an open `Pending::Choice`, read off `ChoiceKind`/
//! `ChoiceOption` rather than off `Move::Choose { n }` alone, since the
//! `Move` carries only a bare index with no meaning outside the decision it
//! answers.
//!
//! # Deliberately NOT counted (see `docs/HUMAN_PLAY.md`'s comparison section
//! for why each one is out of scope, not silently dropped)
//!
//! `corpuscensus.rs`'s `ActionClass::PutBack` has no counterpart at all --
//! there is no "undo a take" [`Move`] in this engine (see [`Move`]'s own
//! variant list), because it is not a rules action, only a BGO client
//! misclick correction. The bot cannot misclick, so its rate is 0 by
//! construction, not by omission; the doc reports it as such rather than as
//! a ratio against the human number.
//!
//! `Move::PopFree`, `Move::BachTheater`, `Move::CancelPact`,
//! `Move::RemoveLeaderYellow`, `Move::Churchill`, the `Defend`/`SendUnit`/
//! `SendBonus`/`SendDiscard`/`SendDone` family, and every `Pending::Choice`
//! kind besides `PactOffer`/`DiscardMilitary` have no `ActionClass` on the
//! human side to match against (either BGO's own text bins them as
//! bookkeeping -- `BachTheater`'s "Johannes Sebastian Bachupgrades" line,
//! `corpuscensus.rs`'s module doc -- or the corpus never developed a class
//! for them at all), so [`classify_move`] returns `None` for them and this
//! file does not invent a class to catch them. `Move::Barbarossa` is the one
//! judgement call in the other direction: it IS a unit build (with an
//! attached, unlogged population increase), so it is folded into
//! `BuildUnit` -- a small, named approximation, not a gap.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use tta::bots::greedy::{build_bots, BotKind, Search, Seat};
use tta::bots::weighted::eval::load_weights;
use tta::bots::weighted::weights::Weights;
use tta::effects;
use tta::game::{self, MOVE_CAP};
use tta::moves::Move;
use tta::state::{ChoiceKind, ChoiceOption, Keyword, Pending};
use tta::CardId;

// ---------------------------------------------------------------------
// Action classes -- same set of names as corpuscensus::ActionClass, minus
// PutBack (structurally impossible, reported separately, not as a rate).
// ---------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
enum BotClass {
    TakeCard,
    BuildBuilding,
    BuildUnit,
    BuildWonderStage,
    IncreasePopulation,
    UpgradeUnit,
    UpgradeProduction,
    DevelopTechnology,
    ElectLeader,
    ChangeGovernment,
    PlayTactic,
    DeclareWar,
    WinWar,
    PlayAggression,
    ProposePact,
    AcceptPact,
    Colonize,
    Discard,
    Bid,
    WinAuction,
    Destroy,
    Disband,
    Pass,
    PlayEvent,
    PlayActionCard,
    EndTurn,
}

impl BotClass {
    const ALL: &'static [BotClass] = &[
        BotClass::TakeCard,
        BotClass::BuildBuilding,
        BotClass::BuildUnit,
        BotClass::BuildWonderStage,
        BotClass::IncreasePopulation,
        BotClass::UpgradeUnit,
        BotClass::UpgradeProduction,
        BotClass::DevelopTechnology,
        BotClass::ElectLeader,
        BotClass::ChangeGovernment,
        BotClass::PlayTactic,
        BotClass::DeclareWar,
        BotClass::WinWar,
        BotClass::PlayAggression,
        BotClass::ProposePact,
        BotClass::AcceptPact,
        BotClass::Colonize,
        BotClass::Discard,
        BotClass::Bid,
        BotClass::WinAuction,
        BotClass::Destroy,
        BotClass::Disband,
        BotClass::Pass,
        BotClass::PlayEvent,
        BotClass::PlayActionCard,
        BotClass::EndTurn,
    ];

    /// The exact label `corpuscensus.rs::ActionClass::label` uses for the
    /// same class, so the two tables line up verbatim when pasted side by
    /// side into the doc.
    fn label(self) -> &'static str {
        match self {
            BotClass::TakeCard => "take card from row",
            BotClass::BuildBuilding => "build building",
            BotClass::BuildUnit => "build unit",
            BotClass::BuildWonderStage => "build wonder stage",
            BotClass::IncreasePopulation => "increase population",
            BotClass::UpgradeUnit => "upgrade unit",
            BotClass::UpgradeProduction => "upgrade production (farm/mine)",
            BotClass::DevelopTechnology => "develop technology",
            BotClass::ElectLeader => "elect leader",
            BotClass::ChangeGovernment => "change government",
            BotClass::PlayTactic => "play tactic",
            BotClass::DeclareWar => "declare war",
            BotClass::WinWar => "win war",
            BotClass::PlayAggression => "play aggression",
            BotClass::ProposePact => "propose pact",
            BotClass::AcceptPact => "accept pact",
            BotClass::Colonize => "colonize",
            BotClass::Discard => "discard",
            BotClass::Bid => "bid",
            BotClass::WinAuction => "win territory auction",
            BotClass::Destroy => "destroy",
            BotClass::Disband => "disband",
            BotClass::Pass => "pass",
            BotClass::PlayEvent => "play event",
            BotClass::PlayActionCard => "play action card",
            BotClass::EndTurn => "end turn (player-turn denominator)",
        }
    }
}

/// Classifies a move that is a genuine, self-contained player choice --
/// everything EXCEPT the four state-diff-detected classes documented in this
/// file's module doc (`PlayEvent`, `WinAuction`, `Colonize`, `WinWar`) and
/// the two `Pending::Choice`-answering classes (`AcceptPact`, `Discard`),
/// which [`classify_choose`] and the per-move diffs in [`play_one`] handle
/// instead. `None` for every `Move` with no [`BotClass`] counterpart -- see
/// the module doc's "deliberately not counted" section for the full list.
fn classify_move(mv: Move) -> Option<BotClass> {
    use Move::*;
    match mv {
        Take { .. } => Some(BotClass::TakeCard),
        Build { card } => Some(build_or_unit(card)),
        Develop { .. } => Some(BotClass::DevelopTechnology),
        Upgrade { to, .. } => Some(upgrade_class(to)),
        WonderStep { .. } => Some(BotClass::BuildWonderStage),
        Pop => Some(BotClass::IncreasePopulation),
        Revolution { .. } => Some(BotClass::ChangeGovernment),
        PlayLeader { .. } => Some(BotClass::ElectLeader),
        PlayAction { .. } => Some(BotClass::PlayActionCard),
        Destroy { card } => Some(destroy_or_disband(card)),
        PlayTactic { .. } | CopyTactic { .. } => Some(BotClass::PlayTactic),
        Aggression { .. } => Some(BotClass::PlayAggression),
        War { .. } => Some(BotClass::DeclareWar),
        OfferPact { .. } => Some(BotClass::ProposePact),
        // §11.5's no-auction colonization: no WinAuction to pair with it,
        // by construction (no auction happened) -- see the module doc.
        ColumbusColonize { .. } => Some(BotClass::Colonize),
        // The build half of Barbarossa's combo -- see the module doc.
        Barbarossa { .. } => Some(BotClass::BuildUnit),
        Bid { .. } => Some(BotClass::Bid),
        BidPass | PolPass => Some(BotClass::Pass),
        EndTurn => Some(BotClass::EndTurn),
        PopFree | BachTheater { .. } | CancelPact { .. } | PrepareEvent { .. }
        | RemoveLeaderYellow | Defend { .. } | DefendDone | SendUnit { .. }
        | SendBonus { .. } | SendDiscard { .. } | SendDone | Choose { .. }
        | Churchill { .. } | Resign | TradeFoodAsResource | TradeResourceAsFood => None,
    }
}

fn build_or_unit(card: CardId) -> BotClass {
    if card.get().kind.is_unit() {
        BotClass::BuildUnit
    } else {
        BotClass::BuildBuilding
    }
}

fn upgrade_class(to: CardId) -> BotClass {
    if to.get().kind.is_unit() {
        BotClass::UpgradeUnit
    } else {
        BotClass::UpgradeProduction
    }
}

fn destroy_or_disband(card: CardId) -> BotClass {
    if card.get().kind.is_unit() {
        BotClass::Disband
    } else {
        BotClass::Destroy
    }
}

/// Classifies a `Move::Choose { n }` answering `pending`, for the two
/// `ChoiceKind`s that have a human-side class at all. `PactOffer`'s
/// `Keyword::Refuse` branch and every other `ChoiceKind` fall through to
/// `None` -- a decline has no counted class on the human side either
/// (`corpuscensus.rs`'s `"declines"` arm is bookkeeping, not an action).
///
/// `Discard`'s granularity is a named approximation, not an exact match:
/// this counts one `Discard` per `Choose` that answers a `DiscardMilitary`
/// decision. Whether that is one-per-card or one-per-requirement, and
/// whether it lines up with however many "X discards ..." lines BGO emits
/// for the same requirement, was not independently verified for this pass --
/// flagged in `docs/HUMAN_PLAY.md` rather than assumed silently.
fn classify_choose(pending: &Pending, n: u8) -> Option<BotClass> {
    match pending {
        Pending::Choice(choice) => match choice.kind {
            ChoiceKind::PactOffer { .. } => match choice.options.get(n as usize) {
                Some(ChoiceOption::Word(Keyword::Accept)) => Some(BotClass::AcceptPact),
                _ => None,
            },
            ChoiceKind::DiscardMilitary => Some(BotClass::Discard),
            ChoiceKind::GainBlock | ChoiceKind::FreeCivil { .. } | ChoiceKind::FoodOrRes { .. } | ChoiceKind::FreeBuild | ChoiceKind::DestroyOwn | ChoiceKind::LosePop | ChoiceKind::LoseColony | ChoiceKind::FlipWonder | ChoiceKind::Raid { .. } | ChoiceKind::Annex { .. } | ChoiceKind::Infiltrate { .. } | ChoiceKind::TakeRow { .. } | ChoiceKind::WarTech { .. } | ChoiceKind::PlunderSplit { .. } | ChoiceKind::FoodOrResSplit { .. } => None,
        },
        Pending::Auction(_) | Pending::Defense(_) | Pending::Colonize(_) => None,
    }
}

/// `Some(winner)` exactly when `pre` was an in-progress territory auction and
/// `post` no longer is -- the move just applied is the one that settled it (a
/// real `Bid`/`BidPass`, since `interact::start_auction` always requires at
/// least one before an auction can resolve, so this signal can never be
/// skipped by auto-play the way `Pending::Colonize` can be -- see the module
/// doc). `None` if the auction closed with nobody having bid at all
/// (`corpuscensus.rs` sees this too: a territory revealed with no eligible
/// bidder logs no colonize either), or if it is still open.
fn auction_just_won(pre: &Option<Pending>, post: &Option<Pending>) -> Option<u8> {
    let Some(Pending::Auction(a)) = pre else { return None };
    if matches!(post, Some(Pending::Auction(_))) {
        return None; // still bidding
    }
    a.high
}

/// The victor of a war whose `war_declared_by_me` field just cleared on
/// `attacker`, given the two sides' strength AS COMPUTED FROM THE STATE
/// BEFORE the clearing move (matching `combat::resolve_war_outcome`'s own
/// read timing -- see the module doc). `None` on a tie, mirroring that
/// function's own `if a == d { return None }`.
fn war_victor(attacker: u8, defender: u8, attacker_strength: i32, defender_strength: i32) -> Option<u8> {
    use std::cmp::Ordering::*;
    match attacker_strength.cmp(&defender_strength) {
        Greater => Some(attacker),
        Less => Some(defender),
        Equal => None,
    }
}

// ---------------------------------------------------------------------
// Per-game / per-run tally
// ---------------------------------------------------------------------

#[derive(Default)]
struct Bucket {
    games: u64,
    turns: u64,
    action_counts: HashMap<BotClass, u64>,
}

impl Bucket {
    fn add(&mut self, c: BotClass, n: u64) {
        *self.action_counts.entry(c).or_insert(0) += n;
    }

    fn count(&self, c: BotClass) -> u64 {
        self.action_counts.get(&c).copied().unwrap_or(0)
    }

    fn per_game(&self, c: BotClass) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.count(c) as f64 / self.games as f64
        }
    }

    fn per_turn(&self, c: BotClass) -> f64 {
        if self.turns == 0 {
            0.0
        } else {
            self.count(c) as f64 / self.turns as f64
        }
    }

    fn merge(&mut self, other: &Bucket) {
        self.games += other.games;
        self.turns += other.turns;
        for (&c, &n) in &other.action_counts {
            *self.action_counts.entry(c).or_insert(0) += n;
        }
    }
}

/// Play one game with every seat at `weights` (a self-play mirror match,
/// the same convention `climb`'s arena and `rust_league.sh` use to measure a
/// champion -- `climb.rs` plays its champion as `BotKind::Weighted`, which is
/// why every seat here is too), tallying every [`BotClass`] this file can
/// detect. Not built on top of `game::play_game` -- that function's callback
/// only gets to CHOOSE a move, and every state-diff class here needs to see
/// the state on both sides of `apply`, so this reimplements its loop with the
/// same shape (`pick` -> `game::step`, capped at [`MOVE_CAP`]) plus the
/// instrumentation.
fn play_one(players: u8, weights: Weights, seed: u64) -> (Bucket, bool) {
    let seats: Vec<Seat> =
        (0..players).map(|_| Seat { kind: BotKind::Weighted, weights, search: Search::None }).collect();
    let mut bots = build_bots(&seats, seed as i64);
    let mut state = game::new_game(players, seed);

    let mut bucket = Bucket::default();
    let mut moves_played = 0usize;
    let mut cap_hit = false;

    while !state.game_over {
        if moves_played >= MOVE_CAP {
            cap_hit = true;
            break;
        }
        let mv = bots[state.current as usize].pick(&state);

        // ---- pre-move snapshots for the diff-based classes ----
        let pre_pending = state.pending.top().cloned();
        let pre_past_events = state.past_events.len();
        // At most one player per move actually resolves a war in this
        // engine (`start_turn` resolves only the incoming player's own),
        // but checking all of them is cheap (<=4) and does not assume that.
        let mut war_snapshots: Vec<(u8, u8, i32, i32)> = Vec::new(); // (attacker, defender, atk_str, def_str)
        for i in 0..players {
            let card = state.players[i as usize].war_declared_by_me;
            if !card.is_none() {
                let target = state.players[i as usize].war_target;
                let atk = effects::state_stats(&state, &state.players[i as usize]).strength;
                let def = effects::state_stats(&state, &state.players[target as usize]).strength;
                war_snapshots.push((i, target, atk, def));
            }
        }

        // ---- classify the move itself ----
        if let Some(class) = classify_move(mv) {
            bucket.add(class, 1);
        }
        if let Move::Choose { n } = mv {
            if let Some(pending) = &pre_pending {
                if let Some(class) = classify_choose(pending, n) {
                    bucket.add(class, 1);
                }
            }
        }
        if mv == Move::EndTurn {
            bucket.turns += 1;
        }

        game::step(&mut state, mv);
        moves_played += 1;

        // ---- post-move diffs ----
        let grew = state.past_events.len().saturating_sub(pre_past_events);
        if grew > 0 {
            bucket.add(BotClass::PlayEvent, grew as u64);
        }

        let post_pending = state.pending.top().cloned();
        if auction_just_won(&pre_pending, &post_pending).is_some() {
            // A won auction always completes into exactly one colonize --
            // see the module doc's "WinAuction / Colonize" section for why
            // this is credited in lockstep rather than detected separately.
            bucket.add(BotClass::WinAuction, 1);
            bucket.add(BotClass::Colonize, 1);
        }

        for (attacker, defender, atk_str, def_str) in war_snapshots {
            let still_at_war = !state.players[attacker as usize].war_declared_by_me.is_none();
            if !still_at_war && war_victor(attacker, defender, atk_str, def_str).is_some() {
                bucket.add(BotClass::WinWar, 1);
            }
        }
    }

    bucket.games = 1;
    (bucket, cap_hit)
}

// ---------------------------------------------------------------------
// CLI / reporting
// ---------------------------------------------------------------------

struct Args {
    games: usize,
    players: u8,
    weights_path: String,
    seed: u64,
    threads: usize,
}

impl Default for Args {
    fn default() -> Args {
        Args { games: 20, players: 3, weights_path: String::new(), seed: 1, threads: 1 }
    }
}

const USAGE: &str = "\
usage: botcensus --weights PATH [options]

  --games N       games to play (default 20)
  --players N     2, 3 or 4 (default 3)
  --weights PATH  champion JSON every seat plays (required)
  --seed N        base seed; game g uses seed+g (default 1)
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
            "--games" => a.games = value(flag)?.parse().map_err(|_| "bad --games".to_string())?,
            "--players" => a.players = value(flag)?.parse().map_err(|_| "bad --players".to_string())?,
            "--weights" => a.weights_path = value(flag)?,
            "--seed" => a.seed = value(flag)?.parse().map_err(|_| "bad --seed".to_string())?,
            "--threads" => a.threads = value(flag)?.parse().map_err(|_| "bad --threads".to_string())?,
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
    if a.weights_path.is_empty() {
        return Err("--weights is required".to_string());
    }
    if a.games == 0 {
        return Err("--games must be at least 1".to_string());
    }
    if a.threads == 0 {
        a.threads = 1;
    }
    Ok(Some(a))
}

fn print_table(name: &str, players: u8, b: &Bucket) {
    println!("\n### {name} ({players}p, n={} games)\n", b.games);
    println!("| action class | /game | /turn |");
    println!("|---|---|---|");
    for &class in BotClass::ALL {
        println!("| {} | {:.3} | {:.4} |", class.label(), b.per_game(class), b.per_turn(class));
    }
    println!("| put card back (take-back upper bound) | N/A | N/A |");
    println!("| **player-turns (denominator)** | {:.2} | -- |", b.turns as f64 / b.games.max(1) as f64);
    println!(
        "\n`put card back` has no engine Move at all -- see the module doc's \
         \"deliberately not counted\" section -- so it is reported as N/A, not 0.000: \
         the bot cannot misclick, this class does not apply to it."
    );
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("botcensus: {e}");
            return ExitCode::FAILURE;
        }
    };

    let weights = match load_weights(std::path::Path::new(&args.weights_path)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("botcensus: loading {}: {e}", args.weights_path);
            return ExitCode::FAILURE;
        }
    };

    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = args.threads.min(args.games);
    let mut results: Vec<Option<(Bucket, bool)>> = (0..args.games).map(|_| None).collect();

    std::thread::scope(|scope| {
        let (slots, args, next, weights) = (&mut results[..], &args, &next, &weights);
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(move || {
                let mut mine = Vec::new();
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= args.games {
                        break;
                    }
                    let seed = args.seed.wrapping_add(i as u64);
                    let r = play_one(args.players, *weights, seed);
                    mine.push((i, r));
                }
                mine
            }));
        }
        for h in handles {
            for (i, r) in h.join().expect("botcensus thread panicked") {
                slots[i] = Some(r);
            }
        }
    });

    let mut overall = Bucket::default();
    let mut capped = 0usize;
    for r in results {
        let (b, cap) = r.expect("every game played");
        capped += usize::from(cap);
        overall.merge(&b);
    }

    let elapsed = start.elapsed().as_secs_f64();
    println!("games        {}", args.games);
    println!("players      {}", args.players);
    println!("weights      {}", args.weights_path);
    println!("seeds        {}..{}", args.seed, args.seed + args.games as u64 - 1);
    println!("elapsed      {elapsed:.1}s  ({:.1} games/s)", args.games as f64 / elapsed.max(1e-9));
    if capped > 0 {
        println!("WARNING      {capped} game(s) hit the {MOVE_CAP}-move cap -- that is a bug, not a long game");
    }

    print_table("Bot action rates", args.players, &overall);

    if capped > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tta::state::{Auction, Choice, OptionList};
    use tta::CardType;

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    fn first_card_of_kind(kind: CardType) -> CardId {
        for i in 0..tta::CARDS.len() {
            let id = CardId(i as u16);
            if id.get().kind == kind {
                return id;
            }
        }
        panic!("no card of kind {kind:?} in CARDS");
    }

    #[test]
    fn take_is_a_take_card() {
        assert_eq!(classify_move(Move::Take { slot: 3 }), Some(BotClass::TakeCard));
    }

    #[test]
    fn building_bronze_is_a_building_not_a_unit() {
        // Bronze is a Mine (production, takes_workers, not is_unit).
        assert_eq!(classify_move(Move::Build { card: card("Bronze") }), Some(BotClass::BuildBuilding));
    }

    #[test]
    fn building_warriors_is_a_unit() {
        assert_eq!(classify_move(Move::Build { card: card("Warriors") }), Some(BotClass::BuildUnit));
    }

    #[test]
    fn develop_is_technology_regardless_of_card_kind() {
        // Matches corpuscensus's own "discovers" arm: kind-agnostic.
        assert_eq!(classify_move(Move::Develop { card: card("Bronze") }), Some(BotClass::DevelopTechnology));
    }

    #[test]
    fn upgrading_to_a_unit_is_upgrade_unit_upgrading_to_a_mine_is_upgrade_production() {
        assert_eq!(
            classify_move(Move::Upgrade { from: card("Warriors"), to: card("Swordsmen") }),
            Some(BotClass::UpgradeUnit)
        );
        assert_eq!(
            classify_move(Move::Upgrade { from: card("Bronze"), to: card("Iron") }),
            Some(BotClass::UpgradeProduction)
        );
    }

    #[test]
    fn wonder_step_of_any_size_is_one_build_wonder_stage() {
        assert_eq!(classify_move(Move::WonderStep { steps: 3 }), Some(BotClass::BuildWonderStage));
    }

    #[test]
    fn pop_is_increase_population_but_pop_free_is_not_counted() {
        assert_eq!(classify_move(Move::Pop), Some(BotClass::IncreasePopulation));
        assert_eq!(classify_move(Move::PopFree), None);
    }

    #[test]
    fn revolution_is_change_government_not_develop_technology() {
        assert_eq!(
            classify_move(Move::Revolution { card: card("Communism") }),
            Some(BotClass::ChangeGovernment)
        );
    }

    #[test]
    fn destroying_a_building_is_destroy_disbanding_a_unit_is_disband() {
        assert_eq!(classify_move(Move::Destroy { card: card("Bronze") }), Some(BotClass::Destroy));
        assert_eq!(classify_move(Move::Destroy { card: card("Warriors") }), Some(BotClass::Disband));
    }

    #[test]
    fn play_tactic_and_copy_tactic_both_count_as_play_tactic() {
        let t = first_card_of_kind(CardType::Tactic);
        assert_eq!(classify_move(Move::PlayTactic { card: t }), Some(BotClass::PlayTactic));
        assert_eq!(classify_move(Move::CopyTactic { card: t }), Some(BotClass::PlayTactic));
    }

    #[test]
    fn offering_a_pact_is_propose_pact() {
        assert_eq!(
            classify_move(Move::OfferPact {
                card: card("Peace Treaty"),
                target: 1,
                side: tta::moves::PactSide::A,
            }),
            Some(BotClass::ProposePact)
        );
    }

    #[test]
    fn columbus_colonize_counts_as_colonize_with_no_matching_auction() {
        assert_eq!(
            classify_move(Move::ColumbusColonize { card: card("Vast Territory (I)") }),
            Some(BotClass::Colonize)
        );
    }

    #[test]
    fn barbarossa_combo_is_folded_into_build_unit() {
        assert_eq!(classify_move(Move::Barbarossa { card: card("Warriors") }), Some(BotClass::BuildUnit));
    }

    #[test]
    fn bid_pass_and_political_pass_both_count_as_pass() {
        assert_eq!(classify_move(Move::BidPass), Some(BotClass::Pass));
        assert_eq!(classify_move(Move::PolPass), Some(BotClass::Pass));
    }

    #[test]
    fn bach_theater_is_not_counted_matching_the_human_census_treating_it_as_bookkeeping() {
        assert_eq!(
            classify_move(Move::BachTheater { from: card("Drama"), to: card("Opera") }),
            None
        );
    }

    #[test]
    fn moves_with_no_human_side_class_return_none() {
        for mv in [
            Move::RemoveLeaderYellow,
            Move::DefendDone,
            Move::SendDone,
            Move::Churchill { choice: tta::moves::ChurchillChoice::Culture },
            Move::Resign,
        ] {
            assert_eq!(classify_move(mv), None, "{mv:?} unexpectedly got a class");
        }
    }

    // ---- Pending::Choice-driven classes ----

    fn accept_refuse_choice(offered: Keyword) -> (Pending, u8) {
        let owner = 0u8;
        let card = card("Peace Treaty");
        let mut options = OptionList::new();
        options.push(ChoiceOption::Word(Keyword::Accept));
        options.push(ChoiceOption::Word(Keyword::Refuse));
        let n = match offered {
            Keyword::Accept => 0,
            Keyword::Stop | Keyword::Skip | Keyword::Science | Keyword::Food | Keyword::Resources | Keyword::Refuse | Keyword::Leader | Keyword::Wonder => 1,
        };
        let choice =
            Choice { player: 1, kind: ChoiceKind::PactOffer { owner, card, a: 0, b: 1 }, options };
        (Pending::Choice(choice), n)
    }

    #[test]
    fn accepting_a_pact_offer_counts_as_accept_pact() {
        let (pending, n) = accept_refuse_choice(Keyword::Accept);
        assert_eq!(classify_choose(&pending, n), Some(BotClass::AcceptPact));
    }

    #[test]
    fn refusing_a_pact_offer_counts_as_nothing_matching_corpuscensus_declines_being_bookkeeping() {
        let (pending, n) = accept_refuse_choice(Keyword::Refuse);
        assert_eq!(classify_choose(&pending, n), None);
    }

    #[test]
    fn a_discard_military_choice_counts_as_discard() {
        let choice = Choice { player: 0, kind: ChoiceKind::DiscardMilitary, options: OptionList::new() };
        assert_eq!(classify_choose(&Pending::Choice(choice), 0), Some(BotClass::Discard));
    }

    #[test]
    fn an_unrelated_choice_kind_counts_as_nothing() {
        let choice = Choice { player: 0, kind: ChoiceKind::GainBlock, options: OptionList::new() };
        assert_eq!(classify_choose(&Pending::Choice(choice), 0), None);
    }

    // ---- auction / war diff detectors ----

    #[test]
    fn an_auction_that_is_still_open_is_not_yet_won() {
        let a = Auction::new(card("Vast Territory (I)"), &[0, 1]);
        let pre = Some(Pending::Auction(a));
        let post = Some(Pending::Auction(a));
        assert_eq!(auction_just_won(&pre, &post), None);
    }

    #[test]
    fn an_auction_that_closes_with_no_bidder_is_not_a_win() {
        let a = Auction::new(card("Vast Territory (I)"), &[0]);
        let pre = Some(Pending::Auction(a));
        // Closed with nobody having bid: pending goes away, `high` stays None.
        assert_eq!(auction_just_won(&pre, &None), None);
    }

    #[test]
    fn an_auction_that_closes_with_a_high_bidder_is_a_win_for_them() {
        let mut a = Auction::new(card("Vast Territory (I)"), &[0, 1]);
        a.high = Some(0);
        a.bid = 3;
        let pre = Some(Pending::Auction(a));
        assert_eq!(auction_just_won(&pre, &None), Some(0));
    }

    #[test]
    fn war_victor_picks_the_stronger_side_and_ties_count_as_nobody() {
        assert_eq!(war_victor(0, 1, 10, 5), Some(0));
        assert_eq!(war_victor(0, 1, 5, 10), Some(1));
        assert_eq!(war_victor(0, 1, 7, 7), None);
    }

    #[test]
    fn every_bot_class_has_a_distinct_label() {
        let mut seen = std::collections::HashSet::new();
        for &c in BotClass::ALL {
            assert!(seen.insert(c.label()), "duplicate label {}", c.label());
        }
        assert_eq!(seen.len(), BotClass::ALL.len());
    }

    #[test]
    fn a_two_player_bot_game_plays_to_completion_and_produces_a_nonzero_tally() {
        // End-to-end smoke test: a real game, not fixtures, so a wiring bug
        // in play_one (e.g. reading the wrong player's pending stack) shows
        // up as an assertion failure here rather than only in the printed
        // report nobody re-reads carefully every run.
        let weights = Weights::default();
        let (bucket, cap_hit) = play_one(2, weights, 42);
        assert!(!cap_hit, "a 2p game should finish well inside the move cap");
        assert_eq!(bucket.games, 1);
        assert!(bucket.turns > 0, "a finished game must have played at least one turn");
        assert!(bucket.count(BotClass::TakeCard) > 0, "a finished game takes cards from the row");
        assert_eq!(bucket.count(BotClass::EndTurn), bucket.turns, "EndTurn is the turn denominator by definition");
    }
}
