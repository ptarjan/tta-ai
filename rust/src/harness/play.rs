//! The operator loop: play the CGE app, log the bot.
//!
//! Built on [`crate::advisor::advisor::Advisor`], which already does the
//! hard part -- mirroring the board, ranking moves, fuzzy card names, never
//! crashing on bad input. What is added here is only what a physical-table
//! session needs on top of the advisor's own REPL:
//!
//! 1. **A minimal, derived prompt.** Opponent turns ask for the freshly
//!    dealt row cards and for anything of theirs that became public (a
//!    completed wonder). Rival state is collected once per round as
//!    [`mirror::RIVAL_ASK_KEYS`] numbers per rival -- the set that
//!    reconstructs every rival-derived feature the evaluator has, no more
//!    and no fewer (see `harness::mirror`'s own doc comment for the proof).
//!    The derivation is re-run every round against the live board
//!    ([`HarnessConsole::rederive`]), so when a feature grows an eye for
//!    something else the harness starts demanding it BY ITSELF.
//! 2. **A mandatory per-round checksum** on our own board, which the
//!    mirror *predicts* and can therefore be caught getting wrong
//!    ([`HarnessConsole::round_check`]).
//! 3. **A structured record** (`harness::record`) with the setup, the pact
//!    bias and the derived observable set attached.
//!
//! It also counts keystrokes, so a cost estimate stops being a guess after
//! the first real game.
//!
//! Ported from `harness/play.py`. The biggest representation change
//! crossing into Rust: Python's scripted test `Operator` reads the live
//! board through a closure that captures the console via a one-element list
//! filled in AFTER construction (`self.ref = console_ref`), a workaround for
//! Python's construction order having no other way to let the callback see
//! state that exists only once the object it will be attached to exists.
//! Rust has no such hazard: [`Io::ask`] simply takes `&Advisor` as a
//! parameter, so anything implementing it (a terminal, or this module's own
//! test double) reads live state the direct way, with no circular
//! reference and no `Option` that is `None` until "later".
//!
//! `HarnessConsole` intentionally does not carry `advisor::Console`'s
//! `more`/`state`/`undo` niceties (`bin/advisor.rs` has no reusable library
//! `Console` to build on -- see that file's own top doc comment for why it
//! is a binary-local REPL, not a `tta::advisor` type) -- `board`/`help`/
//! `quit` plus move entry (Enter, a candidate number, or a typed move) cover
//! what a harness session actually needs.

use std::time::Instant;

use crate::advisor::advisor::{self as advisor, Advisor, Candidate};
use crate::advisor::state_io;
use crate::cards::Age;
use crate::fixtures::Json;
use crate::game;
use crate::harness::fields::{self, ProbeId, Verdict};
use crate::harness::mirror::{self, Value};
use crate::harness::record::{self, GameLog, Source};
use crate::moves::Move;

const BOARD_WIDTH: usize = 100;

/// Everything a driving loop needs to provide: a way to ask a question and
/// get a line back, and a way to print a line. `ask` receives the live
/// [`Advisor`] (board plus bot) so an implementation can read the true state
/// of the game to answer -- a real terminal ignores it, a scripted test
/// operator reads it to answer "perfectly" by construction. Mirrors the
/// `inp`/`out` callables `HarnessConsole.__init__` takes in Python.
pub trait Io {
    fn ask(&mut self, prompt: &str, adv: &Advisor) -> String;
    fn say(&mut self, s: &str);
}

/// `advisor::Console` plus checksums, logging and a keystroke counter.
/// Mirrors `HarnessConsole`.
pub struct HarnessConsole<IO: Io> {
    adv: Advisor,
    log: GameLog,
    io: IO,
    strict: bool,
    keystrokes: u64,
    prompts: u64,
    rounds_checked: u32,
    overrides: u32,
    history: mirror::RivalHistory,
    /// Literal lines typed this round, the forensic trail `observed` keeps.
    typed: Vec<String>,
    checked_round: Option<u16>,
    aborted: bool,
    abort_reason: String,
    verdicts: Vec<(ProbeId, Verdict)>,
    mandatory: Vec<ProbeId>,
    t0: Instant,
    /// The move the advisor actually applied on this "your move" prompt, if
    /// any -- read back by `my_turn` to decide whether to log a decision,
    /// without needing Python's `adv.play` monkey-patch (there is nothing to
    /// patch onto in Rust; `handle_move_input` just sets this directly after
    /// a successful `Advisor::play`).
    last_move: Option<Move>,
}

/// A quit reaches [`HarnessConsole::run`] from ANY prompt through this,
/// mirroring the Rust advisor console's own `Quit` (see `bin/advisor.rs`'s
/// top doc comment for the Python inconsistency that convention closes).
struct Quit;

impl<IO: Io> HarnessConsole<IO> {
    pub fn new(adv: Advisor, log: GameLog, io: IO, strict: bool, verdicts: Vec<(ProbeId, Verdict)>) -> HarnessConsole<IO> {
        let mandatory = fields::requirements(&verdicts, fields::PROBES).into_iter().filter(|r| r.mandatory()).map(|r| r.probe.id).collect();
        HarnessConsole {
            adv,
            log,
            io,
            strict,
            keystrokes: 0,
            prompts: 0,
            rounds_checked: 0,
            overrides: 0,
            history: mirror::RivalHistory::new(),
            typed: Vec::new(),
            checked_round: None,
            aborted: false,
            abort_reason: String::new(),
            verdicts,
            mandatory,
            t0: Instant::now(),
            last_move: None,
        }
    }

    // ---- io, counted

    fn ask(&mut self, prompt: &str) -> String {
        self.prompts += 1;
        let line = self.io.ask(prompt, &self.adv);
        self.keystrokes += line.len() as u64 + 1; // + the Enter
        if !line.trim().is_empty() {
            self.typed.push(line.trim().to_string());
        }
        line
    }

    fn say(&mut self, s: &str) {
        self.io.say(s);
    }

    fn report(&mut self, line: &str) {
        let trimmed = line.trim();
        let body = if trimmed.len() >= 4 && trimmed[..4].eq_ignore_ascii_case("set ") { trimmed[4..].trim() } else { trimmed };
        match self.adv.patch(body) {
            Ok(msg) if !msg.is_empty() => self.say(&format!("    ok: {msg}")),
            Ok(_) => {}
            Err(e) => self.say(&format!("    ! {e}  (type '?' for the syntax, or use '?' as a value if you don't know it)")),
        }
    }

    // ---- new cards, shared between our turn and an opponent's

    fn check_dealt(&mut self) -> Result<(), Quit> {
        if self.adv.dealt_slots.is_empty() {
            return Ok(());
        }
        let slots = self.adv.dealt_slots.clone();
        let where_: Vec<String> = slots.iter().map(|s| s.to_string()).collect();
        self.say(&format!("\n{} new card(s) in row {} {}.", slots.len(), if slots.len() == 1 { "slot" } else { "slots" }, where_.join(", ")));
        loop {
            let line = self.ask("  new cards (left to right, '?' if unseen)> ");
            let line = line.trim().to_string();
            let low = line.to_lowercase();
            if matches!(low.as_str(), "quit" | "q" | "exit") {
                return Err(Quit);
            }
            if matches!(low.as_str(), "help" | "h" | "?") && line != "?" {
                self.say(state_io::PATCH_HELP);
                continue;
            }
            if line.is_empty() || line == "?" {
                self.adv.dealt_slots.clear();
                return Ok(());
            }
            if advisor::looks_like_patch(&line) {
                self.report(&line);
                continue;
            }
            let names = state_io::split_cards(&line, Some(state_io::Pool::Row));
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

    // ------------------------------------------------------- our own turn

    fn my_turn(&mut self) -> Result<bool, Quit> {
        self.check_dealt()?;
        if !self.round_check()? {
            return Ok(false);
        }
        while self.adv.my_turn() && !self.adv.state().game_over {
            let cands = self.adv.recommend(40);
            if cands.is_empty() {
                return Ok(true);
            }
            let top = cands.len().min(3);
            self.show_candidates(&cands[..top]);
            let state_text = state_io::dumps(&self.adv.board);
            let st_round = self.adv.state().round;
            let st_age = self.adv.state().age_civil;
            let t = Instant::now();
            let line = self.ask("your move> ");
            let line = line.trim().to_string();
            let latency = t.elapsed().as_secs_f64();
            self.last_move = None;
            self.handle_move_input(&line, &cands[..top])?;
            if self.last_move.is_some() {
                self.log_decision(&state_text, &cands, &line, st_round, st_age, latency);
            }
        }
        Ok(true)
    }

    fn show_candidates(&mut self, cands: &[Candidate]) {
        let (round, age, ca, ma, food, res, sci) = {
            let st = self.adv.state();
            let p = st.actor();
            (st.round, st.age_civil, p.civil_actions, p.military_actions, p.food, p.resources, p.science)
        };
        self.say(&format!(
            "\n-- your turn (round {round}, age {}): CA {ca}, MA {ma}, food {food}, res {res}, sci {sci}",
            state_io::age_str(age)
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
            self.say(&header_text());
            return Ok(());
        }
        if low == "board" {
            self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));
            return Ok(());
        }
        if advisor::looks_like_patch(line) {
            self.report(line);
            return Ok(());
        }
        let mv = if line.is_empty() {
            cands.first().map(|c| c.mv)
        } else if let Ok(n) = line.parse::<usize>() {
            match n.checked_sub(1).and_then(|i| cands.get(i)) {
                Some(c) => Some(c.mv),
                None => {
                    self.say(&format!("  ! no candidate {n}"));
                    return Ok(());
                }
            }
        } else {
            match advisor::parse_move(self.adv.state(), line, Some(&self.adv.board)) {
                Ok(mv) => Some(mv),
                Err(e) => {
                    self.say(&format!("  ! {e}"));
                    None
                }
            }
        };
        let Some(mv) = mv else { return Ok(()) };
        let (ok, msg) = self.adv.play(mv);
        if ok {
            self.last_move = Some(mv);
        }
        self.say(&format!("{}{}", if ok { "  -> " } else { "  ! " }, msg));
        Ok(())
    }

    fn log_decision(&mut self, state_text: &str, cands: &[Candidate], raw: &str, round_: u16, age: Age, latency: f64) {
        let ranked: Vec<Json> = cands
            .iter()
            .map(|c| {
                Json::obj(vec![
                    ("move", Json::Str(format!("{:?}", c.mv))),
                    ("score", Json::Num((c.score * 1e5).round() / 1e5)),
                    ("text", Json::Str(c.text.clone())),
                    ("reason", Json::Str(c.reason.clone())),
                ])
            })
            .collect();
        let top = cands.first().map(|c| c.mv);
        let last = self.last_move.expect("log_decision called without a move having been played");
        let source = if Some(last) == top { Source::Bot } else { Source::Human };
        if source == Source::Human {
            self.overrides += 1;
        }
        let note = if source == Source::Bot { String::new() } else { format!("typed {raw:?}") };
        self.log.decision(
            state_text,
            ranked,
            Some(format!("{last:?}")),
            source,
            round_,
            state_io::age_str(age),
            Some((latency * 10.0).round() / 10.0),
            &note,
        );
        if self.strict && source == Source::Human {
            self.say("  ** OVERRIDE recorded. In strict mode this game no longer measures the bot; say so in the result notes.");
        }
    }

    // ------------------------------------------------------ opponent turns

    fn opponent_turn(&mut self) -> Result<bool, Quit> {
        let who = self.adv.state().decider();
        self.check_dealt()?;
        self.say(&format!("\n-- p{who}'s turn."));
        let mut line = self.ask(&format!("  did p{who} hit YOU, complete a WONDER, or change the shared board? (Enter = no) > "));
        let mut line_trim = line.trim().to_string();
        while !line_trim.is_empty() {
            let low = line_trim.to_lowercase();
            if matches!(low.as_str(), "quit" | "q" | "exit") {
                return Err(Quit);
            }
            if matches!(low.as_str(), "help" | "?") {
                self.say(state_io::PATCH_HELP);
            } else {
                self.report(&line_trim);
            }
            line = self.ask("  > ");
            line_trim = line.trim().to_string();
        }
        let _ = line; // silence "unused after last assignment" on some toolchains
        self.adv.skip_opponent_turn();
        Ok(true)
    }

    // ------------------------------------------------------- the checksum

    /// Once per round, right before our turn: does the mirror match the
    /// app? This is the only thing standing between an hour of work and a
    /// silently fabricated game, so it is not skippable and there is no
    /// "ignore". Mirrors `round_check`.
    fn round_check(&mut self) -> Result<bool, Quit> {
        let round = self.adv.state().round;
        if self.checked_round == Some(round) {
            return Ok(true);
        }
        self.checked_round = Some(round);
        self.rounds_checked += 1;
        self.rederive();
        self.say(&format!("\n== round {round} check (age {}) -- read the app, do not guess", state_io::age_str(self.adv.state().age_civil)));
        let reported = self.ask_self()?;
        let rivals = self.ask_rivals()?;
        let result = mirror::round_check(&self.adv.board, &reported, Some(&mut self.history), &rivals);
        let state_text = state_io::dumps(&self.adv.board);
        self.log.check(&result, Some(&state_text));
        for d in &result.discrepancies {
            if d.severity == mirror::Severity::Warn {
                self.say(&format!("  ? {d}  (arithmetic, not proof -- re-read it)"));
            }
        }
        if result.failed() && !self.handle_desync(&result, &rivals)? {
            return Ok(false);
        }
        // Only now, and only the forced keys: `w`/`col`/`wip` are checks on
        // what the mirror already believes, so writing them back would
        // erase the check.
        for (idx, vals) in &rivals {
            for key in mirror::RIVAL_FORCE_KEYS {
                if let Some(v) = get_int(vals, key.as_str()) {
                    self.report(&format!("p{idx} {}={v}", key.as_str()));
                }
            }
        }
        let dealt = self.adv.dealt_slots.clone();
        self.log.observed(round, &dealt, &rivals, &self.typed);
        self.typed.clear();
        Ok(true)
    }

    /// Say out loud which asked fields this bot does not currently read.
    /// Mirrors `ask_set_note`.
    fn ask_set_note(&self) -> String {
        let idle: Vec<&str> = mirror::RIVAL_ASK_KEYS
            .iter()
            .filter(|k| {
                let pid = k.probe_id();
                let v = self.verdicts.iter().find(|(id, _)| *id == pid).map(|&(_, v)| v).unwrap_or(Verdict::Inert);
                !v.is_mandatory()
            })
            .map(|k| k.as_str())
            .collect();
        if idle.is_empty() {
            "every rival field you are asked for moves this bot's move.".to_string()
        } else {
            format!(
                "note: {} are ADVISORY for this bot -- they do not change the move it plays today. You are \
                 typing them so the position can be re-scored by the bot that finishes training next.",
                idle.join(", ")
            )
        }
    }

    /// Re-run the field derivation against the live board. Free (one extra
    /// scoring pass per probe) and it is what keeps this harness correct
    /// across evaluator changes: the day a new feature lands, the affected
    /// probe stops being advisory and the operator is told, mid-game, that
    /// the rules of engagement changed. Mirrors `_rederive`.
    fn rederive(&mut self) {
        let fresh = fields::probe_position(self.adv.state(), self.adv.board.me, &self.adv.bot.weights, fields::PROBES);
        for (pid, v) in fresh {
            match self.verdicts.iter_mut().find(|(id, _)| *id == pid) {
                Some(entry) => entry.1 = entry.1.worse(v),
                None => self.verdicts.push((pid, v)),
            }
        }
        let now: Vec<ProbeId> = fields::requirements(&self.verdicts, fields::PROBES).into_iter().filter(|r| r.mandatory()).map(|r| r.probe.id).collect();
        let mut new: Vec<&str> = now.iter().filter(|id| !self.mandatory.contains(id)).map(|id| id.as_str()).collect();
        if !new.is_empty() {
            new.sort_unstable();
            self.say(&format!("\n  ** THE BOT'S EYES CHANGED. It now reads: {}", new.join(", ")));
            self.say("  ** These must be transcribed from here on, and games logged before this point saw less. See `harness fields`.");
        }
        self.mandatory = now;
    }

    fn ask_self(&mut self) -> Result<Vec<(String, Value)>, Quit> {
        let spine = mirror::SPINE.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("/");
        loop {
            let line = self.ask(&format!("  you   {spine} > "));
            let line = line.trim().to_string();
            let low = line.to_lowercase();
            if matches!(low.as_str(), "quit" | "q" | "exit") {
                return Err(Quit);
            }
            if low == "board" {
                self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));
                continue;
            }
            let (reported, errs) = mirror::parse_self_line(&line);
            let missing = mirror::missing_spine(&reported, mirror::SPINE);
            if !errs.is_empty() {
                for e in &errs {
                    self.say(&format!("    ! {e}"));
                }
                continue;
            }
            if !missing.is_empty() {
                self.say(&format!("    ! still need {} -- the check is the point of this exercise, not paperwork", missing.join(", ")));
                continue;
            }
            return Ok(reported);
        }
    }

    fn ask_rivals(&mut self) -> Result<Vec<(u8, Vec<(String, Value)>)>, Quit> {
        let me = self.adv.board.me;
        let n = self.adv.state().num_players as usize;
        let idxs: Vec<u8> = self.adv.state().players[..n].iter().filter(|q| q.idx != me && !q.resigned).map(|q| q.idx).collect();
        let ask = mirror::RIVAL_ASK_KEYS.iter().map(|k| k.as_str()).collect::<Vec<_>>();
        let ask_str = ask.join("/");
        let mut rivals = Vec::new();
        for idx in idxs {
            loop {
                let line = self.ask(&format!("  p{idx}    {ask_str} > "));
                let line = line.trim().to_string();
                let low = line.to_lowercase();
                if matches!(low.as_str(), "quit" | "q" | "exit") {
                    return Err(Quit);
                }
                let (vals, errs) = mirror::parse_line(&line, &ask, &ask);
                if !errs.is_empty() {
                    for e in &errs {
                        self.say(&format!("    ! {e}"));
                    }
                    continue;
                }
                rivals.push((idx, vals));
                break;
            }
        }
        Ok(rivals)
    }

    /// A hard mismatch. There is no "continue anyway" option, by design.
    /// Mirrors `handle_desync`.
    fn handle_desync(&mut self, result: &mirror::CheckResult, rivals: &[(u8, Vec<(String, Value)>)]) -> Result<bool, Quit> {
        self.say(&format!("\n{}", "!".repeat(70)));
        self.say("DESYNC. The mirror and the app disagree:");
        for d in &result.discrepancies {
            if d.severity == mirror::Severity::Fail {
                self.say(&format!("   {d}"));
            }
        }
        self.say("The bot has been choosing moves in a position that is not the");
        self.say("one on your screen, for an unknown number of plies.");
        self.say("  [a] abort  -- log it and stop. Right answer for early games.");
        self.say("  [r] resync -- type corrections, then say what caused it.");
        self.say(&"!".repeat(70));
        loop {
            let choice = self.ask("  a/r> ");
            let choice = choice.trim().to_lowercase();
            match choice.chars().next() {
                Some('a') => {
                    self.aborted = true;
                    self.abort_reason =
                        result.discrepancies.iter().filter(|d| d.severity == mirror::Severity::Fail).map(|d| d.to_string()).collect::<Vec<_>>().join("; ");
                    self.log.resync(result.round, &result.discrepancies, &[], "aborted, not resynced");
                    return Ok(false);
                }
                Some('r') => {
                    self.say("  type corrections (blank line to finish):");
                    let mut patches = Vec::new();
                    loop {
                        let line = self.ask("   > ");
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            break;
                        }
                        patches.push(line.clone());
                        self.report(&line);
                    }
                    let mut cause = self.ask("  what caused it? (required; 'unknown' is a valid answer) > ");
                    while cause.trim().is_empty() {
                        cause = self.ask("  a cause is required > ");
                    }
                    self.log.resync(result.round, &result.discrepancies, &patches, cause.trim());
                    let after = mirror::round_check(&self.adv.board, &result.reported, None, rivals);
                    if after.failed() {
                        self.say("  ! still out of sync:");
                        for d in &after.discrepancies {
                            self.say(&format!("     {d}"));
                        }
                        continue;
                    }
                    self.say("  resynced. This game is now UNTRUSTED (record.trusted=false): keep it, do not pool it.");
                    return Ok(true);
                }
                _ => continue,
            }
        }
    }

    // --------------------------------------------------------- the ending

    pub fn run(&mut self) -> Json {
        self.say(&header_text());
        let note = self.ask_set_note();
        self.say(&note);
        self.say(&format!("bot: {}", self.adv.bot_source));
        self.say(&state_io::render(&self.adv.board, BOARD_WIDTH));
        if let Err(Quit) = self.play_loop() {
            self.aborted = true;
            if self.abort_reason.is_empty() {
                self.abort_reason = "operator quit".to_string();
            }
        }
        // Stopping early is not a result. Recording it as one would put a
        // half-played game into the aggregate with a real-looking margin.
        if !self.adv.state().game_over && !self.aborted {
            self.aborted = true;
            self.abort_reason = "stopped before the game ended".to_string();
        }
        self.finish()
    }

    fn play_loop(&mut self) -> Result<(), Quit> {
        loop {
            if self.adv.state().game_over {
                return Ok(());
            }
            let cont = if self.adv.my_turn() { self.my_turn()? } else { self.opponent_turn()? };
            if !cont {
                return Ok(());
            }
        }
    }

    fn finish(&mut self) -> Json {
        let game_over = self.adv.state().game_over;
        let round = self.adv.state().round;
        let mut scores: Vec<(String, i64)> = Vec::new();
        if game_over {
            let raw = game::scores(self.adv.state());
            scores = raw.into_iter().enumerate().map(|(i, s)| (format!("p{i}"), s as i64)).collect();
        }
        let mut notes: Vec<String> = Vec::new();
        if !self.aborted {
            self.say("\nFinal scores AS THE APP SHOWS THEM (these, not the mirror's, are the measurement).");
            let real = self.ask_scores();
            if !real.is_empty() {
                if !scores.is_empty() && real != scores {
                    notes.push(format!("mirror scores {scores:?} != app scores {real:?}"));
                }
                scores = real;
            }
        }
        let minutes_line = self.ask("  wall-clock minutes (Enter = measured) > ");
        let human_minutes = match minutes_line.trim().parse::<i64>() {
            Ok(m) => Some(m),
            Err(_) => Some((self.t0.elapsed().as_secs_f64() / 60.0).round().max(1.0) as i64),
        };
        let free = self.ask("  notes (misreads, overrides, anything odd) > ");
        if !free.trim().is_empty() {
            notes.push(free.trim().to_string());
        }
        if self.overrides > 0 {
            notes.push(format!("{} override(s) of the bot's top pick", self.overrides));
        }
        let effort = self.effort(human_minutes);
        let observables: Vec<Json> = fields::requirements(&self.verdicts, fields::PROBES).iter().map(|r| r.to_json()).collect();
        let rec = self.log.result(&scores, round, human_minutes, &notes.join("; "), self.aborted, &self.abort_reason, effort, observables);
        let path = self.log.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        self.say(&format!("\nwrote {path}"));
        let trusted = rec.get("trusted").and_then(Json::as_bool).unwrap_or(false);
        let flag = if trusted { String::new() } else { format!(" ({})", rec.get("untrusted_reason").and_then(Json::as_str).unwrap_or("")) };
        self.say(&format!("  trusted={trusted}{flag}"));
        if let Some(e) = rec.get("effort") {
            let ks = e.get("keystrokes").and_then(Json::as_f64).unwrap_or(0.0);
            let pr = e.get("prompts").and_then(Json::as_f64).unwrap_or(0.0);
            let rn = e.get("rounds").and_then(Json::as_f64).unwrap_or(0.0);
            let kpr = e.get("keystrokes_per_round").and_then(Json::as_f64).unwrap_or(0.0);
            self.say(&format!("  {ks:.0} keystrokes / {pr:.0} prompts over {rn:.0} rounds = {kpr:.0} keystrokes per round"));
        }
        self.say("  REMINDER: no pacts were possible in this game -- the CGE AI refuses them. This is not a full-game measurement.");
        rec
    }

    fn ask_scores(&mut self) -> Vec<(String, i64)> {
        let n = self.adv.state().num_players;
        let prompt = format!("  final scores  {} > ", (0..n).map(|i| format!("p{i}")).collect::<Vec<_>>().join("/"));
        let line = self.ask(&prompt);
        let toks: Vec<String> = line.replace(',', " ").replace('/', " ").split_whitespace().map(str::to_string).collect();
        toks.iter().enumerate().filter_map(|(i, t)| t.parse::<i64>().ok().map(|v| (format!("p{i}"), v))).collect()
    }

    fn effort(&self, human_minutes: Option<i64>) -> Json {
        let r = self.rounds_checked.max(1) as f64;
        Json::obj(vec![
            ("keystrokes", Json::Num(self.keystrokes as f64)),
            ("prompts", Json::Num(self.prompts as f64)),
            ("rounds", Json::Num(self.rounds_checked as f64)),
            ("overrides", Json::Num(self.overrides as f64)),
            ("keystrokes_per_round", Json::Num(self.keystrokes as f64 / r)),
            ("minutes_per_round", Json::Num(human_minutes.unwrap_or(0) as f64 / r)),
        ])
    }
}

fn get_int(vals: &[(String, Value)], key: &str) -> Option<i64> {
    vals.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
        Value::Int(n) => Some(*n),
        Value::Text(_) => None,
    })
}

/// The banner printed once at the start of a session. Mirrors `HEADER`.
pub fn header_text() -> String {
    let spine = mirror::SPINE.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("/");
    let rival = mirror::RIVAL_ASK_KEYS.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("/");
    let fields_help: String = mirror::RIVAL_ASK_KEYS.iter().map(|k| format!("    {:<4} {}", k.as_str(), k.label())).collect::<Vec<_>>().join("\n");
    format!(
        "Through the Ages -- CGE app harness.\n\n\
Per round you type this, and nothing else:\n\
  * the cards dealt into the row, as you see them ('bro irr alc' is enough)\n\
  * one line for your own panel     {spine}\n\
  * one line per rival panel        {rival}\n\
  * the NAME of any wonder a rival completes, when it happens\n\
      p1 built+ Colossus\n\n\
{fields_help}\n\n\
Everything else on their boards -- techs, government, food, resources,\n\
military actions, free workers, happiness -- is provably invisible to the\n\
evaluator (`harness fields` prints the derivation). Do not type it. If the\n\
evaluator grows an eye for something new, this harness will start asking for\n\
it on its own, mid-game, in capital letters.\n\n\
Every number above is public information at the table (RULES_SPEC.md 2.6 for\n\
the hand: civil cards are only ever taken from the open row). If you find\n\
yourself opening a hidden zone to answer one of them, stop -- that is a bug\n\
in this harness, not a thing you should type.\n\n\
STRICT MODE IS THE MEASUREMENT. Press Enter and play what the bot starred.\n\
The moment you \"help\" it, the score stops measuring the bot -- and only the\n\
note you write at the end will ever say so.\n"
    )
}

// ------------------------------------------------------------------- setup

/// The pre-flight, driven by a plain ask/say pair rather than [`Io`] (there
/// is no board yet to hand a scripted test double). Refuses to start a game
/// we cannot honestly log. Mirrors `ask_setup`.
pub fn ask_setup(
    mut ask: impl FnMut(&str) -> String,
    mut say: impl FnMut(&str),
    game_id: Option<String>,
    players: u8,
    seat: u8,
    difficulty: record::Difficulty,
    free: bool,
    dlc: Option<bool>,
    app_version: String,
    weights: String,
    operator: String,
    personalities: Vec<String>,
) -> Result<record::Setup, String> {
    let dlc = match dlc {
        Some(d) => d,
        None => {
            say(
                "\nIs 'New Leaders & Wonders' switched OFF in the app's game setup? Our engine does not \
                 implement it, so a DLC game is a mislabelled log, not a weak one.",
            );
            !ask("  expansion is OFF? [y/N] > ").trim().to_lowercase().starts_with('y')
        }
    };
    let mut setup = record::Setup::new(game_id.unwrap_or_else(|| now_id()));
    setup.players = players;
    setup.seat = seat;
    setup.difficulty = difficulty;
    setup.mode = if free { record::Mode::Free } else { record::Mode::Strict };
    setup.dlc = dlc;
    setup.app_version = app_version;
    setup.weights = weights;
    setup.operator = operator;
    setup.personalities = personalities;
    setup.validate()?;
    Ok(setup)
}

fn now_id() -> String {
    let dur = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("game-{}", dur.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisor::advisor::load_bot;
    use crate::advisor::state_io as S;
    use crate::state::ROW_SIZE;
    use std::path::Path;

    /// A scripted human. Answers prompts by reading the live [`Advisor`]
    /// passed into `ask`, so by construction it is in perfect sync unless
    /// `drift`/`rival_drift` say otherwise. Mirrors Python's `Operator`.
    struct ScriptedOperator {
        stop_round: u16,
        drift: Vec<(u16, Vec<(&'static str, i64)>)>,
        rival_drift: Vec<(u16, Vec<(&'static str, i64)>)>,
        mv: String,
        overrides_left: u32,
        pub lines: Vec<String>,
        pub out: Vec<String>,
        /// One-shot answers to the "a/r>" prompt, consumed front-first
        /// before falling back to repeating `desync_answer` forever --
        /// lets a test feed junk ("i", "ignore", "") ahead of the real
        /// choice to prove there is no third way through the prompt.
        desync_queue: std::collections::VecDeque<String>,
        desync_answer: String,
        cause: String,
        resync_fix: Option<(&'static str, i64)>,
        resync_sent: bool,
    }

    impl ScriptedOperator {
        fn new() -> ScriptedOperator {
            ScriptedOperator {
                stop_round: 4,
                drift: Vec::new(),
                rival_drift: Vec::new(),
                mv: String::new(),
                overrides_left: 0,
                lines: Vec::new(),
                out: Vec::new(),
                desync_queue: std::collections::VecDeque::new(),
                desync_answer: "a".to_string(),
                cause: "misread the app".to_string(),
                resync_fix: None,
                resync_sent: false,
            }
        }
    }

    impl Io for ScriptedOperator {
        fn ask(&mut self, prompt: &str, adv: &Advisor) -> String {
            self.lines.push(prompt.to_string());
            let p = prompt.trim();
            if p.contains("new cards") {
                return "?".to_string();
            }
            let rival_join = mirror::RIVAL_ASK_KEYS.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("/");
            if p.contains(&rival_join) {
                let idx: u8 = p.split_whitespace().next().and_then(|t| t.strip_prefix('p')).and_then(|d| d.parse().ok()).unwrap_or(0);
                let q = &adv.board.state.players[idx as usize];
                let s = crate::effects::compute(&adv.board.state, q);
                let mut vals: Vec<(&str, i64)> = vec![
                    ("c", q.culture as i64),
                    ("cr", s.culture as i64),
                    ("sr", s.science as i64),
                    ("str", s.strength as i64),
                    ("ca", q.civil_actions as i64),
                    ("hc", q.hand_size_civil() as i64),
                    ("w", q.completed_wonders.len() as i64),
                    ("s", q.science as i64),
                    ("f", q.food as i64),
                    ("r", q.resources as i64),
                    ("fw", q.workers_free as i64),
                    ("y", q.yellow_bank as i64),
                    ("ma", q.military_actions as i64),
                    ("col", q.colonies.len() as i64),
                    ("wip", if q.wonder.is_none() { 0 } else { 1 }),
                ];
                if let Some((_, deltas)) = self.rival_drift.iter().find(|(r, _)| *r == adv.board.state.round) {
                    for &(k, d) in deltas {
                        if let Some(entry) = vals.iter_mut().find(|(vk, _)| *vk == k) {
                            entry.1 += d;
                        }
                    }
                }
                return mirror::RIVAL_ASK_KEYS.iter().map(|k| vals.iter().find(|(vk, _)| *vk == k.as_str()).unwrap().1.to_string()).collect::<Vec<_>>().join("/");
            }
            let spine_join = mirror::SPINE.iter().map(|k| k.as_str()).collect::<Vec<_>>().join("/");
            if p.contains(&spine_join) {
                let mut snap = mirror::self_snapshot(&adv.board, mirror::SPINE);
                if let Some((_, deltas)) = self.drift.iter().find(|(r, _)| *r == adv.board.state.round) {
                    for &(k, d) in deltas {
                        if let Some(entry) = snap.iter_mut().find(|(vk, _)| vk == k) {
                            if let Value::Int(n) = &mut entry.1 {
                                *n += d;
                            }
                        }
                    }
                }
                return mirror::SPINE
                    .iter()
                    .map(|k| match &snap.iter().find(|(vk, _)| vk == k.as_str()).unwrap().1 {
                        Value::Int(n) => n.to_string(),
                        Value::Text(s) => s.clone(),
                    })
                    .collect::<Vec<_>>()
                    .join("/");
            }
            if prompt.starts_with("  a/r> ") {
                return self.desync_queue.pop_front().unwrap_or_else(|| self.desync_answer.clone());
            }
            if p.contains("what caused it") || p.contains("a cause is required") {
                return self.cause.clone();
            }
            // The resync correction loop's prompt ("   > ", three spaces) is
            // textually distinct from the opponent-turn follow-up ("  > ",
            // two spaces) -- so, unlike the Python original (which loses
            // that distinction to `.strip()` and has to track extra state to
            // recover it), matching the RAW prompt disambiguates them for
            // free.
            if prompt == "   > " {
                if let (Some((key, delta)), false) = (self.resync_fix, self.resync_sent) {
                    self.resync_sent = true;
                    let me = &adv.board.state.players[adv.board.me as usize];
                    let cur = mirror::self_snapshot(&adv.board, &[SelfKeyFor(key)]);
                    let Value::Int(base) = cur[0].1 else { unreachable!() };
                    let _ = me;
                    return format!("p{} {key}={}", adv.board.me, base + delta);
                }
                return String::new();
            }
            if p.contains("hit YOU") {
                return String::new();
            }
            if prompt == "your move> " {
                if adv.board.state.round >= self.stop_round {
                    return "quit".to_string();
                }
                if self.overrides_left > 0 && adv.recommend(2).len() > 1 {
                    self.overrides_left -= 1;
                    return self.mv.clone();
                }
                return String::new();
            }
            if p.contains("final scores") {
                return "180 200 150".to_string();
            }
            if p.contains("wall-clock minutes") {
                return "70".to_string();
            }
            if p.contains("notes") {
                return String::new();
            }
            String::new()
        }

        fn say(&mut self, s: &str) {
            self.out.push(s.to_string());
        }
    }

    /// [`mirror::self_snapshot`] takes a slice of keys; this converts a
    /// bare key str used by test fixtures back into the one-element slice
    /// form, so the resync test can ask for "the current value of `f`"
    /// without duplicating [`mirror::SelfCheckKey::as_str`]'s match here.
    #[allow(non_snake_case)]
    fn SelfKeyFor(key: &str) -> mirror::SelfCheckKey {
        mirror::SPINE.iter().copied().find(|k| k.as_str() == key).or_else(|| [mirror::SelfCheckKey::F].into_iter().find(|k| k.as_str() == key)).unwrap()
    }

    fn build(dir: &Path, players: u8, op: ScriptedOperator) -> HarnessConsole<ScriptedOperator> {
        let board = S::new_board(players, 0, 3);
        let (bot, _) = load_bot(players, None);
        let mut adv = Advisor::new(board, bot, "test".to_string());
        adv.dealt_slots = (0..ROW_SIZE).collect();
        let mut setup = record::Setup::new("t");
        setup.players = players;
        let verdicts: Vec<(ProbeId, Verdict)> = fields::PROBES.iter().map(|p| (p.id, Verdict::Inert)).collect();
        let reqs = fields::requirements(&verdicts, fields::PROBES);
        let log = GameLog::new(setup, Some(&dir.join("g.jsonl")), &reqs).unwrap();
        HarnessConsole::new(adv, log, op, true, verdicts)
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("harness-play-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn rec_type(j: &Json) -> Option<&str> {
        j.get("type").and_then(Json::as_str)
    }

    // ------------------------------------------------------- perfect operator

    #[test]
    fn a_synced_game_never_reports_a_desync() {
        let d = tmp("synced");
        let mut con = build(&d, 3, ScriptedOperator::new());
        con.run();
        let kinds: Vec<&str> = con.log.records.iter().filter_map(rec_type).collect();
        assert!(kinds.contains(&"check"));
        assert!(kinds.contains(&"decision"));
        assert!(!kinds.contains(&"resync"));
        assert!(!con.io.out.iter().any(|l| l.contains("DESYNC")));
        let checks: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("check")).collect();
        assert!(!checks.is_empty());
        assert!(checks.iter().all(|c| c.get("ok").and_then(Json::as_bool) == Some(true)));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The whole cost saving: one snapshot a round, not one a move.
    #[test]
    fn it_checks_once_per_round_not_once_per_turn() {
        let d = tmp("once-per-round");
        let mut con = build(&d, 3, ScriptedOperator::new());
        con.run();
        let rounds: Vec<i64> = con.log.records.iter().filter(|r| rec_type(r) == Some("check")).filter_map(|r| r.get("round").and_then(Json::as_f64)).map(|n| n as i64).collect();
        let mut sorted = rounds.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(rounds, sorted);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn decisions_are_logged_with_the_replayable_snapshot() {
        let d = tmp("decisions");
        let mut con = build(&d, 3, ScriptedOperator::new());
        con.run();
        let dec: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("decision")).collect();
        assert!(!dec.is_empty());
        for r in &dec {
            assert!(matches!(r.get("state"), Some(Json::Str(s)) if s.starts_with("tta")));
            assert!(r.get("played").is_some_and(|p| *p != Json::Null));
            assert!(matches!(r.get("source").and_then(Json::as_str), Some("bot") | Some("human")));
        }
        let state = dec[0].get("state").and_then(Json::as_str).unwrap();
        S::loads(state).unwrap();
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn effort_is_measured_not_estimated() {
        let d = tmp("effort");
        let mut con = build(&d, 3, ScriptedOperator::new());
        let rec = con.run();
        let e = rec.get("effort").unwrap();
        assert!(e.get("keystrokes").and_then(Json::as_f64).unwrap() > 0.0);
        assert!(e.get("rounds").and_then(Json::as_f64).unwrap() > 0.0);
        let ks = e.get("keystrokes").and_then(Json::as_f64).unwrap();
        let rn = e.get("rounds").and_then(Json::as_f64).unwrap();
        let kpr = e.get("keystrokes_per_round").and_then(Json::as_f64).unwrap();
        assert!((kpr - ks / rn).abs() < 1e-6);
        let _ = std::fs::remove_dir_all(&d);
    }

    // ------------------------------------------------------- drifting operator

    #[test]
    fn a_drifted_board_stops_the_game() {
        let d = tmp("drift-c");
        let mut op = ScriptedOperator::new();
        op.stop_round = 9;
        op.drift = vec![(3, vec![("c", 11)])];
        let mut con = build(&d, 3, op);
        let rec = con.run();
        assert!(con.io.out.iter().any(|l| l.contains("DESYNC")));
        assert_eq!(rec.get("aborted").and_then(Json::as_bool), Some(true));
        assert_eq!(rec.get("trusted").and_then(Json::as_bool), Some(false));
        assert!(rec.get("abort_reason").and_then(Json::as_str).unwrap().contains('c'));
        let resyncs: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("resync")).collect();
        assert_eq!(resyncs.len(), 1);
        assert_eq!(resyncs[0].get("round").and_then(Json::as_f64), Some(3.0));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The one HARD check on the rival side: an operator who forgot a
    /// `built+` must not be able to play on.
    #[test]
    fn a_missed_rival_wonder_stops_the_game() {
        let d = tmp("drift-w");
        let mut op = ScriptedOperator::new();
        op.stop_round = 9;
        op.rival_drift = vec![(3, vec![("w", 1)])];
        let mut con = build(&d, 3, op);
        let rec = con.run();
        assert!(con.io.out.iter().any(|l| l.contains("DESYNC")));
        assert_eq!(rec.get("aborted").and_then(Json::as_bool), Some(true));
        assert!(rec.get("abort_reason").and_then(Json::as_str).unwrap().contains('w'));
        let bad: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("check") && r.get("ok").and_then(Json::as_bool) == Some(false)).collect();
        let keys: Vec<&str> = bad[0].get("discrepancies").and_then(Json::as_arr).unwrap().iter().filter_map(|d| d.get("key").and_then(Json::as_str)).collect();
        assert_eq!(keys, vec!["w", "w"]); // one per rival
        assert!(bad[0].get("discrepancies").and_then(Json::as_arr).unwrap().iter().all(|d| d.get("severity").and_then(Json::as_str) == Some("fail")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_failing_check_is_in_the_log_with_both_numbers() {
        let d = tmp("drift-str");
        let mut op = ScriptedOperator::new();
        op.stop_round = 9;
        op.drift = vec![(3, vec![("str", 4)])];
        let mut con = build(&d, 3, op);
        con.run();
        let bad: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("check") && r.get("ok").and_then(Json::as_bool) == Some(false)).collect();
        assert_eq!(bad.len(), 1);
        let d0 = &bad[0].get("discrepancies").and_then(Json::as_arr).unwrap()[0];
        assert_eq!(d0.get("key").and_then(Json::as_str), Some("str"));
        assert_eq!(d0.get("severity").and_then(Json::as_str), Some("fail"));
        assert_ne!(d0.get("expected"), d0.get("reported"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resync_demands_a_cause_and_taints_the_game() {
        let d = tmp("resync");
        let mut op = ScriptedOperator::new();
        op.stop_round = 5;
        op.drift = vec![(3, vec![("f", 6)])];
        op.desync_answer = "r".to_string();
        op.cause = "forgot an event".to_string();
        op.resync_fix = Some(("f", 6));
        let mut con = build(&d, 3, op);
        let rec = con.run();
        let resyncs: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("resync")).collect();
        assert_eq!(resyncs.len(), 1);
        assert_eq!(resyncs[0].get("cause").and_then(Json::as_str), Some("forgot an event"));
        assert!(!resyncs[0].get("patches").and_then(Json::as_arr).unwrap().is_empty());
        assert_eq!(rec.get("trusted").and_then(Json::as_bool), Some(false));
        assert!(rec.get("untrusted_reason").and_then(Json::as_str).unwrap().contains("desync"));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Requirement: fail loudly. `i`/`ignore`/a blank line must not be
    /// accepted as a way on -- only `a` or `r` ever end the prompt loop.
    #[test]
    fn there_is_no_ignore_option() {
        let d = tmp("no-ignore");
        let mut op = ScriptedOperator::new();
        op.stop_round = 9;
        op.drift = vec![(3, vec![("c", 11)])];
        op.desync_queue = ["i", "ignore", ""].into_iter().map(str::to_string).collect();
        op.desync_answer = "a".to_string();
        let mut con = build(&d, 3, op);
        let rec = con.run();
        assert_eq!(rec.get("aborted").and_then(Json::as_bool), Some(true));
        // Every junk answer must have re-prompted, not silently continued
        // the game: three "a/r>" prompts for the junk plus the one that
        // finally got "a".
        let arprompts = con.io.lines.iter().filter(|l| l.trim_end() == "a/r>" || l.starts_with("  a/r>")).count();
        assert!(arprompts >= 4, "{arprompts}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // ------------------------------------------------------------- overrides

    #[test]
    fn playing_something_other_than_the_star_is_recorded() {
        let d = tmp("overrides");
        let mut op = ScriptedOperator::new();
        op.stop_round = 3;
        op.mv = "2".to_string();
        op.overrides_left = 2;
        let mut con = build(&d, 3, op);
        let rec = con.run();
        let dec: Vec<&Json> = con.log.records.iter().filter(|r| rec_type(r) == Some("decision")).collect();
        assert!(dec.iter().any(|r| r.get("source").and_then(Json::as_str) == Some("human")), "an override was invisible in the log");
        assert!(con.io.out.iter().any(|l| l.contains("OVERRIDE")));
        assert!(rec.get("notes").and_then(Json::as_str).unwrap().contains("override"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // ---------------------------------------------------- moving target live

    /// When the evaluator gains an eye, the operator is told, loudly,
    /// without anybody editing this harness.
    #[test]
    fn a_new_mandatory_field_is_announced_mid_game() {
        let d = tmp("moving-target");
        let mut op = ScriptedOperator::new();
        op.stop_round = 3;
        let mut con = build(&d, 3, op);
        con.verdicts = fields::PROBES.iter().map(|p| (p.id, Verdict::Inert)).collect();
        con.mandatory = fields::requirements(&con.verdicts, fields::PROBES).into_iter().filter(|r| r.mandatory()).map(|r| r.probe.id).collect();
        con.rederive();
        assert!(con.io.out.iter().any(|l| l.contains("THE BOT'S EYES CHANGED")), "the live derivation never fired");
        let _ = std::fs::remove_dir_all(&d);
    }
}
