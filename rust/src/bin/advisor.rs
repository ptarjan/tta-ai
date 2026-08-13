//! `advisor` -- a text co-pilot for a human playing Through the Ages at a
//! physical table.
//!
//! ```text
//! advisor --players 3 --seat 0
//! ```
//!
//! Every turn it prints the top few moves with a score and a one-line
//! reason, the human presses Enter to accept the top one (or types their
//! own move), and between turns they type short update lines describing
//! what opponents did and which cards were dealt. Nothing typed here
//! crashes it: bad input is explained and re-prompted.
//!
//! The board mirror, the ranking and the move-parsing all live in
//! [`tta::advisor`] so they can be unit-tested without a terminal attached;
//! this file is the command line and the interactive loop over stdin/
//! stdout, nothing else -- the same split `arena.rs`/`climb.rs` keep between
//! the measurement (in the library) and the report (in the binary).
//!
//! Ported from `advisor/advisor.py`'s `Console`/`main`. One behavioural fix
//! made crossing into Rust: Python's `Console` only shows the "bye -- the
//! snapshot below restores this game" recovery text when `quit` is typed at
//! the "new cards dealt" prompt specifically (raised as a `_Quit`
//! exception); typing `quit` at the "your move" or "what happened" prompts
//! instead falls out through a `return False` chain that reaches `main`
//! silently, with no snapshot printed -- a human who quits from the more
//! common prompts loses their restore text. Every prompt here funnels
//! through the same `Result<_, Quit>` path to [`run`], so quitting ANYWHERE
//! always prints the snapshot; see `quitting_from_the_move_prompt_prints_the_recovery_snapshot`.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use tta::advisor::advisor::{self, Advisor, Candidate};
use tta::advisor::state_io;
use tta::game;
use tta::moves::Move;
use tta::state::ROW_SIZE;

// ====================================================================== args

#[derive(Clone, Debug)]
struct Args {
    players: u8,
    seat: u8,
    seed: u64,
    weights: Option<PathBuf>,
    load: Option<PathBuf>,
    save: Option<PathBuf>,
}

impl Default for Args {
    fn default() -> Args {
        Args { players: 3, seat: 0, seed: 0, weights: None, load: None, save: None }
    }
}

const USAGE: &str = "\
usage: advisor [options]

  --players N     2, 3 or 4 (default 3)
  --seat N        your seat, 0 = start player (default 0)
  --seed N        deal seed, only used for a fresh game (default 0)
  --weights PATH  bot weight JSON (default: experiments/rust_champion_<N>p.json,
                   the live league's own output -- gitignored, so a fresh
                   clone falls back to the built-in defaults if that is
                   missing)
  --load PATH     resume from a snapshot file instead of dealing fresh
  --save PATH     one-shot, non-interactive mode instead of the terminal
                   REPL: read update lines from stdin (same grammar as the
                   'what happened' prompt, blank lines and trailing input
                   both fine), apply them, auto-play the bot's own top pick
                   for every action of the resulting turn (printing each
                   move, its score and a one-line reason), then write the
                   resulting position to PATH and exit -- no prompts, no
                   tty required. Exits non-zero, naming the bad line, on an
                   update line that does not parse.
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
            "--players" => a.players = parse_num(&value(flag)?, flag)?,
            "--seat" => a.seat = parse_num(&value(flag)?, flag)?,
            "--seed" => a.seed = parse_num(&value(flag)?, flag)?,
            "--weights" => a.weights = Some(PathBuf::from(value(flag)?)),
            "--load" => a.load = Some(PathBuf::from(value(flag)?)),
            "--save" => a.save = Some(PathBuf::from(value(flag)?)),
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

// ======================================================================= UI

/// A normal terminal width. Python's `render` took no width parameter (one
/// fixed layout); this port's takes one so `state_io`'s tests can render at
/// other widths too, so the console just picks an ordinary one.
const BOARD_WIDTH: usize = 100;

const BANNER_HEAD: &str = "\
Through the Ages advisor.  Commands at the 'your move' prompt:

  <Enter>      play the top recommendation
  1 / 2 / 3    play that numbered recommendation
  take 4       play your own move (verb + fuzzy args), e.g.
               build bronze | dev philosophy | wonder | end | pass
  more         show more candidate moves
  board        print the full board
  state        print the raw snapshot (paste-able)
  p1 c=34      correct the board at any prompt (see the update syntax
               below); 'set <line>' works too
  undo         undo back to the start of your turn
  help         this text
  quit         leave

At the 'what happened' prompt type update lines (blank line = done):
";

fn banner() -> String {
    format!("{BANNER_HEAD}{}", state_io::PATCH_HELP)
}

/// The human asked to leave. Mirrors Python's `_Quit` exception -- every
/// prompt-reading method below returns this instead of a message, and
/// [`Console::run`] is the one place that catches it, so quitting from any
/// prompt reaches the identical recovery text. See this file's top doc
/// comment for the Python inconsistency this closes.
struct Quit;

struct Console {
    adv: Advisor,
    /// The board text at the start of the human's current turn, so `undo`
    /// can roll back to it. Mirrors `Console._snapshot`.
    snapshot: Option<String>,
}

impl Console {
    fn new(adv: Advisor) -> Console {
        Console { adv, snapshot: None }
    }

    fn ask(&self, prompt: &str) -> String {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) => "quit".to_string(), // EOF, mirrors Python catching EOFError
            Ok(_) => line.trim_end_matches(['\n', '\r']).to_string(),
            Err(_) => "quit".to_string(),
        }
    }

    fn say(&self, s: &str) {
        println!("{s}");
    }

    fn run(&mut self) {
        self.say(&banner());
        self.say(&format!("bot: {}", self.adv.bot_source));
        self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));

        let outcome = loop {
            if self.adv.state().game_over {
                break Ok(());
            }
            let step = if self.adv.my_turn() { self.my_turn() } else { self.opponent_turn() };
            if let Err(Quit) = step {
                break Err(Quit);
            }
        };

        match outcome {
            Err(Quit) => {
                self.say("bye -- the snapshot below restores this game:");
                self.say(&state_io::dumps(&self.adv.board));
            }
            Ok(()) => {
                let scores: Vec<String> = game::scores(self.adv.state())
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("p{i}={s}"))
                    .collect();
                self.say(&format!("\ngame over.  final culture: {}", scores.join(", ")));
            }
        }
    }

    // ---- your turn

    fn my_turn(&mut self) -> Result<(), Quit> {
        self.snapshot = Some(state_io::dumps(&self.adv.board));
        self.check_dealt()?;
        while self.adv.my_turn() && !self.adv.state().game_over {
            let cands = self.adv.recommend(3);
            if cands.is_empty() {
                return Ok(());
            }
            self.show_candidates(&cands);
            let line = self.ask("your move> ");
            self.handle_move_input(line.trim(), &cands)?;
        }
        Ok(())
    }

    fn show_candidates(&self, cands: &[Candidate]) {
        let st = self.adv.state();
        let p = st.actor();
        self.say(&format!(
            "\n-- your turn (round {}, age {:?}): CA {}, MA {}, food {}, res {}, sci {}",
            st.round, st.age_civil, p.civil_actions, p.military_actions, p.food, p.resources, p.science
        ));
        for (i, c) in cands.iter().enumerate() {
            let n = i + 1;
            let mark = if n == 1 { "*" } else { " " };
            let gap = if n == 1 { String::new() } else { format!("  ({:+.1})", c.score) };
            self.say(&format!(" {mark}{n}. {}{gap}", c.text));
            self.say(&format!("       why: {}", c.reason));
        }
    }

    fn handle_move_input(&mut self, line: &str, cands: &[Candidate]) -> Result<(), Quit> {
        let low = line.to_lowercase();
        if matches!(low.as_str(), "quit" | "q" | "exit") {
            return Err(Quit);
        }
        if matches!(low.as_str(), "help" | "?" | "h") {
            self.say(&banner());
            return Ok(());
        }
        if low == "board" {
            self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));
            return Ok(());
        }
        if low == "state" {
            self.say(&state_io::dumps(&self.adv.board));
            return Ok(());
        }
        if low == "more" {
            for c in self.adv.recommend(10).iter().skip(3) {
                self.say(&format!("    - {}  ({:+.1})  why: {}", c.text, c.score, c.reason));
            }
            return Ok(());
        }
        if low == "undo" {
            if let Some(snap) = self.snapshot.clone() {
                match state_io::loads(&snap) {
                    Ok(board) => {
                        self.adv.board = board;
                        self.say("rolled back to the start of your turn");
                    }
                    Err(e) => self.say(&format!("  ! could not restore: {e}")),
                }
            }
            return Ok(());
        }
        if advisor::looks_like_patch(line) {
            self.report(line);
            return Ok(());
        }

        let mv = if line.is_empty() {
            cands[0].mv
        } else if let Ok(n) = line.parse::<usize>() {
            match n.checked_sub(1).and_then(|i| cands.get(i)) {
                Some(c) => c.mv,
                None => {
                    self.say(&format!("  ! no candidate {n}"));
                    return Ok(());
                }
            }
        } else {
            match advisor::parse_move(self.adv.state(), line, Some(&self.adv.board)) {
                Ok(mv) => mv,
                Err(e) => {
                    self.say(&format!("  ! {e}"));
                    return Ok(());
                }
            }
        };

        let (ok, msg) = self.adv.play(mv);
        self.say(&format!("{}{}", if ok { "  -> " } else { "  ! " }, msg));
        // `end_turn` no longer finishes the turn on its own: §6.6 step 1 is
        // the player's decision (which military card to discard), so the
        // sequence can suspend instead of advancing. Only announce the turn
        // as over once the engine has actually run it out -- mirrors the
        // same check in Python's `handle_move_input`.
        let turn_settled = self.adv.state().pending.is_empty();
        if ok && matches!(mv, Move::EndTurn) && turn_settled {
            self.after_my_turn()?;
        } else if ok
            && matches!(mv, Move::Choose { .. })
            && turn_settled
            && self.adv.state().current != self.adv.board.me
        {
            self.after_my_turn()?;
        }
        Ok(())
    }

    fn after_my_turn(&mut self) -> Result<(), Quit> {
        self.say(
            "\nyour turn is over.  Anything to correct on YOUR board (military cards drawn, event effects)?",
        );
        self.collect_updates()
    }

    // ---- opponents

    fn opponent_turn(&mut self) -> Result<(), Quit> {
        let who = self.adv.state().decider();
        self.check_dealt()?;
        self.say(&format!(
            "\n-- p{who}'s turn.  Tell me what they did (blank line when done, 'help' for the syntax):"
        ));
        self.collect_updates()?;
        self.adv.skip_opponent_turn();
        Ok(())
    }

    fn collect_updates(&mut self) -> Result<(), Quit> {
        loop {
            let line = self.ask("  > ");
            let line = line.trim();
            if line.is_empty() {
                return Ok(());
            }
            let low = line.to_lowercase();
            if matches!(low.as_str(), "quit" | "q" | "exit") {
                return Err(Quit);
            }
            if matches!(low.as_str(), "help" | "?") {
                self.say(state_io::PATCH_HELP);
                continue;
            }
            if low == "board" {
                self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));
                continue;
            }
            self.report(line);
        }
    }

    fn report(&mut self, line: &str) {
        let trimmed = line.trim();
        let body = if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("set ") {
            trimmed[4..].trim()
        } else {
            trimmed
        };
        match self.adv.patch(body) {
            Ok(msg) if !msg.is_empty() => self.say(&format!("    ok: {msg}")),
            Ok(_) => {}
            Err(e) => self.say(&format!(
                "    ! {e}  (type '?' for the syntax, or use '?' as a value if you don't know it)"
            )),
        }
    }

    /// Ask which cards were actually dealt into the row. Mirrors
    /// `check_dealt`. Python also records `board.unknown.add("row.new")`
    /// when the human answers `?`; grepping the whole tree shows nothing
    /// ever reads that key back, and Rust's `Board.unknown` is a typed
    /// `BTreeSet<UnknownField>` keyed to one player's one field (the whole
    /// point of `state_io.rs`'s design is closing the "stringly-typed
    /// unknown" hole), so there is no type-safe slot to carry a fact nothing
    /// consumes -- dropped rather than reinvented.
    fn check_dealt(&mut self) -> Result<(), Quit> {
        if self.adv.dealt_slots.is_empty() {
            return Ok(());
        }
        let slots = self.adv.dealt_slots.clone();
        let where_: Vec<String> = slots.iter().map(|s| s.to_string()).collect();
        self.say(&format!(
            "\n{} new card(s) in row {} {}.",
            slots.len(),
            if slots.len() == 1 { "slot" } else { "slots" },
            where_.join(", ")
        ));
        loop {
            let line = self.ask("  new cards (left to right, '?' if unseen)> ");
            let line = line.trim();
            let low = line.to_lowercase();
            if matches!(low.as_str(), "quit" | "q" | "exit") {
                return Err(Quit);
            }
            if matches!(low.as_str(), "help" | "h") {
                self.say(state_io::PATCH_HELP);
                continue;
            }
            if low == "board" {
                self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));
                continue;
            }
            if line.is_empty() || line == "?" {
                self.adv.dealt_slots.clear();
                return Ok(());
            }
            if advisor::looks_like_patch(line) {
                // The human is already reporting the rest of the turn.
                self.report(line);
                continue;
            }
            let names = state_io::split_cards(line, Some(state_io::Pool::Row));
            match self.adv.set_dealt(&names) {
                Ok(got) => {
                    let names: Vec<&str> = got.iter().map(|c| c.name()).collect();
                    self.say(&format!("    ok: {}", names.join(", ")));
                    return Ok(());
                }
                Err(e) => self.say(&format!("    ! {e}")),
            }
        }
    }
}

// --------------------------------------------------------- one-shot mode

/// One non-interactive exchange: apply every update line in `input`
/// (`state_io::patch_all`'s grammar -- `row ...`, `p1 str=.. c=..`, and
/// everything else `PATCH_HELP` documents, though `docs/DISCORD_PLAY.md`
/// only asks a Discord operator to ever type the 5 observables that stayed
/// live in front of the bot's evaluation), then advance the mirror past any
/// opponents whose turn it now is, then play out MY whole turn by always
/// taking the top-ranked candidate -- there is no one at a keyboard to ask,
/// so the "recommendation" and "the move actually played" are the same
/// thing here, unlike the interactive REPL's `Console::my_turn`. Returns the
/// full human-readable report (every action's candidates + reason) to print,
/// or an error naming the first line that failed to parse.
///
/// Guarded at 40 iterations in both loops -- the same bound
/// `Advisor::skip_opponent_turn` already uses -- so a bug that stops the
/// decider ever advancing is a clean failure here instead of a hang.
/// Hand the turn back to us, then overwrite the mirror with what Paul reports.
///
/// The order matters and is the whole reason this is a named function. The
/// update lines describe the table as it stands right now -- after the rival
/// moved and after the start-of-turn replenish. Patching before the advance
/// would let the engine's own sweep-and-refill run on top of the corrected
/// row: at 2p that discards the three leftmost cards, slides every survivor
/// left and deals three cards nobody can see, so every slot number we print
/// is off by three and the cheap end of the row is filled with guesses.
fn sync_to_my_turn(adv: &mut advisor::Advisor, input: &str) -> Result<Vec<String>, String> {
    // Two passes, in the order `state_io::PatchTiming` documents. An `event`
    // line names a card the engine is ABOUT to reveal and resolve during the
    // advance below, so it lands first; everything else describes the table
    // once the advance is over, so it lands last.
    let (before, after) = state_io::split_by_timing(input);
    let (mut msgs, errs) = state_io::patch_all(&mut adv.board, &before);
    if !errs.is_empty() {
        return Err(format!("bad update line(s):\n  {}", errs.join("\n  ")));
    }

    let mut guard = 0;
    while !adv.my_turn() && !adv.state().game_over && guard < 40 {
        guard += 1;
        adv.skip_opponent_turn();
    }
    if guard == 40 {
        return Err("opponents never finished handing the turn back to me (40-turn guard hit)".to_string());
    }

    let (rest, errs) = state_io::patch_all(&mut adv.board, &after);
    if !errs.is_empty() {
        return Err(format!("bad update line(s):\n  {}", errs.join("\n  ")));
    }
    msgs.extend(rest);
    Ok(msgs)
}

fn run_batch(adv: &mut advisor::Advisor, input: &str) -> Result<String, String> {
    let msgs = sync_to_my_turn(adv, input)?;

    let mut out = String::new();
    for m in &msgs {
        out.push_str(&format!("ok: {m}\n"));
    }

    let mut step = 0;
    while adv.my_turn() && !adv.state().game_over && step < 40 {
        step += 1;
        let cands = adv.recommend(3);
        if cands.is_empty() {
            break;
        }
        let st = adv.state();
        let p = st.actor();
        out.push_str(&format!(
            "\n-- move {step} (round {}, age {:?}): CA {}, MA {}, food {}, res {}, sci {}\n",
            st.round, st.age_civil, p.civil_actions, p.military_actions, p.food, p.resources, p.science
        ));
        for (i, c) in cands.iter().enumerate() {
            let n = i + 1;
            let mark = if n == 1 { "*" } else { " " };
            let gap = if n == 1 { String::new() } else { format!("  ({:+.1})", c.score) };
            out.push_str(&format!(" {mark}{n}. {}{gap}\n", c.text));
            out.push_str(&format!("       why: {}\n", c.reason));
        }
        let (ok, msg) = adv.play(cands[0].mv);
        if !ok {
            return Err(format!("internal error: the bot's own top pick was rejected: {msg}"));
        }
    }
    if step == 40 {
        return Err("my turn never settled (40-action guard hit)".to_string());
    }

    if adv.state().game_over {
        let scores: Vec<String> =
            game::scores(adv.state()).iter().enumerate().map(|(i, s)| format!("p{i}={s}")).collect();
        out.push_str(&format!("\ngame over.  final culture: {}\n", scores.join(", ")));
    }
    Ok(out)
}

// ------------------------------------------------------------------- main

fn build_board(args: &Args) -> Result<state_io::Board, String> {
    match &args.load {
        Some(path) => {
            let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            state_io::loads(&text)
        }
        None => Ok(state_io::new_board(args.players, args.seat, args.seed)),
    }
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("advisor: {e}");
            return ExitCode::FAILURE;
        }
    };

    let board = match build_board(&args) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("advisor: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (bot, source) = advisor::load_bot(board.state.num_players, args.weights.as_deref());
    let mut adv = Advisor::new(board, bot, source);

    if let Some(save_path) = &args.save {
        // Batch mode: `run_batch` sources the whole board from `--load` (a
        // fresh deal has no physical row to correct against yet -- the
        // caller's first update line is expected to be `row ...`), so unlike
        // the interactive branch below there is no `dealt_slots` priming.
        let mut input = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut input) {
            eprintln!("advisor: failed to read update lines from stdin: {e}");
            return ExitCode::FAILURE;
        }
        return match run_batch(&mut adv, &input) {
            Ok(report) => {
                print!("{report}");
                match std::fs::write(save_path, state_io::dumps(&adv.board)) {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("advisor: {}: {e}", save_path.display());
                        ExitCode::FAILURE
                    }
                }
            }
            Err(e) => {
                eprintln!("advisor: {e}");
                ExitCode::FAILURE
            }
        };
    }

    if args.load.is_none() {
        // The physical row was dealt by the real deck; take it from the
        // human rather than trusting the engine's own (unseen) deal.
        adv.dealt_slots = (0..ROW_SIZE).collect();
    }
    Console::new(adv).run();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_advisor(seed: u64) -> Advisor {
        let board = state_io::new_board(2, 0, seed);
        let (bot, source) = advisor::load_bot(2, None);
        Advisor::new(board, bot, source)
    }

    // ------------------------------------------------------------ run_batch

    #[test]
    fn an_unparseable_update_line_is_reported_by_name_and_stops_before_any_move_is_played() {
        let mut adv = fresh_advisor(1);
        let before_decider = adv.state().decider();
        let err = run_batch(&mut adv, "p1 c=34\nnonsense line here\n").unwrap_err();
        assert!(err.contains("nonsense line here"), "{err}");
        // `patch_all` (a pre-existing, shared library function) applies each
        // update line as it parses, so the earlier `p1 c=34` line does land
        // even though the batch as a whole errors -- that partial apply is
        // fine because `main` never reaches `--save` on an `Err`, so nothing
        // half-applied is ever persisted. What must never happen is the bot
        // auto-playing a move on top of a batch it just rejected.
        assert_eq!(adv.state().decider(), before_decider, "no move should be auto-played after a bad line");
    }

    #[test]
    fn a_fresh_game_where_it_is_already_my_turn_plays_out_without_any_opponent_update() {
        let mut adv = fresh_advisor(1);
        let report = run_batch(&mut adv, "row Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze").unwrap();
        assert!(report.contains("move 1"), "{report}");
        // The turn only ends by running out of legal non-`EndTurn` moves or
        // by the bot itself choosing to end it -- either way the mirror must
        // have handed the decider on to the other seat by the time we save.
        assert_ne!(adv.state().decider(), adv.board.me);
    }

    #[test]
    fn the_reported_row_is_the_one_the_bot_scores_and_is_never_replenished_on_top_of() {
        let all_bronze = "row Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze";
        let mut adv = fresh_advisor(3);

        // The first exchange ends our turn, so the second one has to hand the
        // board back to us -- and that advance is where the start-of-turn
        // replenish lives. Only the second exchange can expose the ordering.
        run_batch(&mut adv, all_bronze).unwrap();
        assert!(!adv.my_turn(), "the first exchange should have passed the board on");

        // Stop at the sync rather than running the whole exchange: once the
        // bot starts playing, ending our turn advances the mirror into the
        // rival's start-of-turn replenish, which legitimately refills the row.
        // The position we care about is the one the bot is about to score.
        sync_to_my_turn(&mut adv, all_bronze).unwrap();

        // Applying the report BEFORE the advance let the replenish run on top
        // of it: at 2p that sweeps the three leftmost slots, slides every
        // survivor left and deals three cards from the engine's own deck. The
        // bot would then score -- and we would print slot numbers for -- a row
        // that is not the one in front of Paul.
        for (i, c) in adv.state().card_row.iter().enumerate() {
            assert!(
                c.get().name == "Bronze",
                "slot {i} holds '{}', which was never reported",
                c.get().name
            );
        }
    }

    #[test]
    fn saving_and_loading_a_board_produces_the_identical_recommended_move() {
        let adv = fresh_advisor(7);
        let before = adv.recommend(3);
        assert!(!before.is_empty());

        let dumped = state_io::dumps(&adv.board);
        let loaded = state_io::loads(&dumped).unwrap();
        let (bot2, source2) = advisor::load_bot(2, None);
        let adv2 = Advisor::new(loaded, bot2, source2);
        let after = adv2.recommend(3);

        assert_eq!(before.len(), after.len());
        for (b, a) in before.iter().zip(after.iter()) {
            assert_eq!(b.mv, a.mv, "recommended move changed across a save/load round trip");
            assert_eq!(b.score, a.score);
            assert_eq!(b.text, a.text);
            assert_eq!(b.reason, a.reason);
        }
    }

    #[test]
    fn round_tripping_a_played_turn_through_save_and_load_keeps_the_next_recommendation_identical() {
        const FIRST_ROW: &str =
            "row Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze Bronze";
        // It is now the rival's turn in the mirror; report their result the
        // same way a Discord operator would (military strength + culture,
        // two of the five observables that are per-rival) and land back on
        // my own turn before comparing recommendations.
        const RIVAL_UPDATE: &str = "row Warriors Warriors Warriors Warriors Warriors Warriors Warriors Warriors Warriors Warriors Warriors Warriors Warriors\np1 str=1 c=0";

        let mut a = fresh_advisor(3);
        run_batch(&mut a, FIRST_ROW).unwrap();
        run_batch(&mut a, RIVAL_UPDATE).unwrap();
        let saved = state_io::dumps(&a.board);

        let loaded = state_io::loads(&saved).unwrap();
        let (bot2, source2) = advisor::load_bot(2, None);
        let via_save_load = Advisor::new(loaded, bot2, source2);

        let mut direct = fresh_advisor(3);
        run_batch(&mut direct, FIRST_ROW).unwrap();
        run_batch(&mut direct, RIVAL_UPDATE).unwrap();

        assert_eq!(via_save_load.recommend(3)[0].mv, direct.recommend(3)[0].mv);
        assert_eq!(via_save_load.recommend(3)[0].score, direct.recommend(3)[0].score);
    }

    // -------------------------------------------------------------- parse_args

    #[test]
    fn save_flag_is_parsed_into_args() {
        let argv: Vec<String> =
            ["--players", "2", "--save", "/tmp/does-not-matter.json"].iter().map(|s| s.to_string()).collect();
        let parsed = parse_args(&argv).unwrap().unwrap();
        assert_eq!(parsed.save, Some(PathBuf::from("/tmp/does-not-matter.json")));
    }
}
