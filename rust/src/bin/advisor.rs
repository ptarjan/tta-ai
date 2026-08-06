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

use std::io::Write;
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
}

impl Default for Args {
    fn default() -> Args {
        Args { players: 3, seat: 0, seed: 0, weights: None, load: None }
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
    if args.load.is_none() {
        // The physical row was dealt by the real deck; take it from the
        // human rather than trusting the engine's own (unseen) deal.
        adv.dealt_slots = (0..ROW_SIZE).collect();
    }
    Console::new(adv).run();
    ExitCode::SUCCESS
}
