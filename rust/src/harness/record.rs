//! The machine-readable per-game record.
//!
//! Append-only JSONL, flushed after every write, so a game abandoned in
//! round 9 still leaves everything up to round 9 on disk.
//!
//! Two things in here are not book-keeping and must not be trimmed:
//!
//! * [`Setup`] -- player count, difficulty, and whether the New Leaders &
//!   Wonders DLC was off. Our engine does not implement the DLC, so a DLC
//!   game is not a weak measurement, it is a MISLABELLED one:
//!   [`Setup::validate`] refuses to open a log for a game that is not the
//!   game we implement.
//! * [`limitations`] -- repeated on the header AND the result record,
//!   because whoever reads the result later may never see the header. The
//!   load-bearing one is the pact bias: CGE's AI never offers a pact and
//!   refuses every pact offered, so the entire pact branch of our policy is
//!   dead weight in these games. A win rate measured here is a win rate on
//!   a strictly smaller game than the one we train on, and nobody may
//!   report it otherwise.
//!
//! Ported from `harness/record.py`. Two representation changes crossing
//! into Rust:
//!
//! * `difficulty`/`mode` are enums ([`Difficulty`]/[`Mode`]), not strings --
//!   an invalid one is now unrepresentable rather than a runtime
//!   `SetupError`. The equivalent of Python's "must be one of {...}" check
//!   moves to the CLI parsing boundary ([`Difficulty::parse`]/[`Mode::
//!   parse`]), the one place a string still has to become one of these.
//! * There is no `serde`/`serde_json` (`Cargo.toml`'s `[dependencies]` stays
//!   empty), so every record is built directly as a [`crate::fixtures::
//!   Json`] value and written with that module's own writer.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use crate::fixtures::{parse_json, Json};
use crate::harness::fields;
use crate::harness::mirror;

pub const SCHEMA: u32 = 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Difficulty {
    Training,
    Easy,
    Medium,
    Hard,
}

pub const DIFFICULTIES: &[Difficulty] = &[Difficulty::Training, Difficulty::Easy, Difficulty::Medium, Difficulty::Hard];

impl Difficulty {
    pub fn as_str(self) -> &'static str {
        match self {
            Difficulty::Training => "training",
            Difficulty::Easy => "easy",
            Difficulty::Medium => "medium",
            Difficulty::Hard => "hard",
        }
    }

    pub fn parse(s: &str) -> Result<Difficulty, String> {
        DIFFICULTIES
            .iter()
            .copied()
            .find(|d| d.as_str() == s)
            .ok_or_else(|| format!("difficulty must be one of {}, got {s:?}", DIFFICULTIES.iter().map(|d| d.as_str()).collect::<Vec<_>>().join("/")))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Strict,
    Free,
}

pub const MODES: &[Mode] = &[Mode::Strict, Mode::Free];

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Strict => "strict",
            Mode::Free => "free",
        }
    }

    pub fn parse(s: &str) -> Result<Mode, String> {
        MODES.iter().copied().find(|m| m.as_str() == s).ok_or_else(|| format!("mode must be one of strict/free, got {s:?}"))
    }
}

/// One game's setup, and the gate that decides whether it may be logged at
/// all. Mirrors the `Setup` dataclass. Every field is public: this is a
/// plain value the caller fills in and then calls [`Setup::validate`] on,
/// the same shape as the Python dataclass plus a `.validate()` call rather
/// than a builder.
#[derive(Clone, Debug)]
pub struct Setup {
    pub game_id: String,
    pub players: u8,
    pub seat: u8,
    pub difficulty: Difficulty,
    pub mode: Mode,
    /// New Leaders & Wonders -- MUST be `false`.
    pub dlc: bool,
    pub edition: String,
    pub src: String,
    pub app_version: String,
    pub platform: Option<String>,
    pub weights: String,
    pub operator: String,
    /// "World leader" AIs -- a different experiment from the headline
    /// difficulty measurement.
    pub personalities: Vec<String>,
    pub notes: String,
}

impl Setup {
    pub fn new(game_id: impl Into<String>) -> Setup {
        Setup {
            game_id: game_id.into(),
            players: 3,
            seat: 0,
            difficulty: Difficulty::Hard,
            mode: Mode::Strict,
            dlc: false,
            edition: "2015-base".to_string(),
            src: "cge-app".to_string(),
            app_version: String::new(),
            platform: None,
            weights: String::new(),
            operator: String::new(),
            personalities: Vec::new(),
            notes: String::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.dlc {
            return Err(
                "New Leaders & Wonders is ON. Our engine does not implement it (docs/EXTERNAL_AIS.md \
                 section 1); this game cannot be logged. Restart the app game with the expansion off."
                    .to_string(),
            );
        }
        if !(2..=4).contains(&self.players) {
            return Err(format!("players must be 2-4, got {}", self.players));
        }
        if self.seat >= self.players {
            return Err(format!("seat {} is not in a {}-player game", self.seat, self.players));
        }
        if !self.personalities.is_empty() && self.difficulty != Difficulty::Hard {
            return Err(
                "'world leader' personalities are a different experiment; label them, do not mix them \
                 into the headline difficulty measurement"
                    .to_string(),
            );
        }
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![
            ("game_id", Json::Str(self.game_id.clone())),
            ("players", Json::Num(self.players as f64)),
            ("seat", Json::Num(self.seat as f64)),
            ("difficulty", Json::Str(self.difficulty.as_str().to_string())),
            ("mode", Json::Str(self.mode.as_str().to_string())),
            ("dlc", Json::Bool(self.dlc)),
            ("edition", Json::Str(self.edition.clone())),
            ("src", Json::Str(self.src.clone())),
            ("app_version", Json::Str(self.app_version.clone())),
            ("platform", Json::Str(self.platform.clone().unwrap_or_else(default_platform))),
            ("weights", Json::Str(self.weights.clone())),
            ("operator", Json::Str(self.operator.clone())),
            ("personalities", Json::Arr(self.personalities.iter().map(|s| Json::Str(s.clone())).collect())),
            ("notes", Json::Str(self.notes.clone())),
        ])
    }
}

fn default_platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// The bias register. Each entry says what a number from this harness does
/// NOT measure. Mirrors `_limitations`.
fn limitations(setup: &Setup) -> Vec<Json> {
    let no_pacts_text = if setup.players > 2 {
        "CGE's AI never offers a pact and refuses every pact offered, so the entire pact branch of our \
         policy is never exercised, rewarded or punished in these games. This result measures the bot \
         on a STRICTLY SMALLER GAME than self-play. Any pact-related weight must be validated by \
         self-play only."
    } else {
        "Pacts are disabled at 2 players by the rules, so the pact branch of our policy is untested \
         here -- as it is in any 2p game."
    };
    let mut out = vec![
        Json::obj(vec![
            ("id", Json::Str("no_pacts".to_string())),
            ("severity", Json::Str(if setup.players > 2 { "high" } else { "low" }.to_string())),
            ("text", Json::Str(no_pacts_text.to_string())),
        ]),
        Json::obj(vec![
            ("id", Json::Str("single_opponent_policy".to_string())),
            ("severity", Json::Str("medium".to_string())),
            (
                "text",
                Json::Str(
                    "Every opponent is the same AI at the same difficulty, so opponent-model variance is \
                     zero. Do not read the margin as a population estimate."
                        .to_string(),
                ),
            ),
        ]),
        Json::obj(vec![
            ("id", Json::Str("no_dlc".to_string())),
            ("severity", Json::Str("info".to_string())),
            (
                "text",
                Json::Str(
                    "Base 2015 game only. New Leaders & Wonders is not implemented by our engine and \
                     must be off."
                        .to_string(),
                ),
            ),
        ]),
    ];
    if setup.mode == Mode::Free {
        out.push(Json::obj(vec![
            ("id", Json::Str("free_mode".to_string())),
            ("severity", Json::Str("high".to_string())),
            (
                "text",
                Json::Str(
                    "FREE MODE: the human chose the moves. The score measures the human, not the bot. \
                     Only the override rate is a signal about the bot."
                        .to_string(),
                ),
            ),
        ]));
    }
    out
}

fn git_rev() -> Option<String> {
    let out = std::process::Command::new("git").args(["rev-parse", "--short", "HEAD"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// `strftime("%Y-%m-%dT%H:%M:%SZ")` on the current UTC time, dependency-free
/// (no `chrono`/`time` crate) via Howard Hinnant's `civil_from_days`
/// algorithm -- see http://howardhinnant.github.io/date_algorithms.html.
/// This timestamp is written once per record and never parsed back by
/// anything in this crate, so a hand-rolled formatter for it is the right
/// amount of machinery, not a shortcut.
fn now_iso8601() -> String {
    let dur = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let secs = dur.as_secs() as i64;
    let days = secs.div_euclid(86400);
    let sod = secs.rem_euclid(86400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", sod / 3600, (sod % 3600) / 60, sod % 60)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Where a logged decision came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Bot,
    Human,
    Forced,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Bot => "bot",
            Source::Human => "human",
            Source::Forced => "forced",
        }
    }
}

/// One game, one file. Every write is flushed. Mirrors `GameLog`.
pub struct GameLog {
    pub setup: Setup,
    pub path: Option<PathBuf>,
    pub records: Vec<Json>,
    pub resyncs: Vec<u16>,
    pub ply: u32,
    file: Option<std::fs::File>,
    closed: bool,
}

impl GameLog {
    pub fn new(setup: Setup, path: Option<&Path>, requirements: &[fields::Requirement]) -> Result<GameLog, String> {
        setup.validate()?;
        let mut file = None;
        if let Some(p) = path {
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
                }
            }
            file = Some(
                std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| format!("{}: {e}", p.display()))?,
            );
        }
        let mut log =
            GameLog { setup, path: path.map(|p| p.to_path_buf()), records: Vec::new(), resyncs: Vec::new(), ply: 0, file, closed: false };
        let header = Json::obj(vec![
            ("v", Json::Num(SCHEMA as f64)),
            ("type", Json::Str("game".to_string())),
            ("id", Json::Str(log.setup.game_id.clone())),
            ("started", Json::Str(now_iso8601())),
            ("engine_rev", git_rev().map(Json::Str).unwrap_or(Json::Null)),
            ("setup", log.setup.to_json()),
            ("limitations", Json::Arr(limitations(&log.setup))),
            // What the bot could actually SEE in this game, derived from the
            // live evaluator rather than assumed (`harness::fields`).
            // Without this a re-scored log is uninterpretable: you would not
            // know whether a field is absent because it did not matter or
            // because nobody typed it.
            ("observables", Json::Arr(requirements.iter().map(|r| r.to_json()).collect())),
        ]);
        log.write(header);
        Ok(log)
    }

    fn write(&mut self, rec: Json) -> Json {
        self.records.push(rec.clone());
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{}", rec.to_string());
            let _ = f.flush();
        }
        rec
    }

    /// One of *our* decisions. `ranked` is the full candidate list, already
    /// turned into JSON by the caller (`harness::play`'s `Candidate` list),
    /// which keeps this module free of a dependency on `advisor::Candidate`.
    pub fn decision(
        &mut self,
        state_text: &str,
        ranked: Vec<Json>,
        played: Option<String>,
        source: Source,
        round_: u16,
        age: &str,
        latency_s: Option<f64>,
        note: &str,
    ) -> Json {
        self.ply += 1;
        let rec = Json::obj(vec![
            ("v", Json::Num(SCHEMA as f64)),
            ("type", Json::Str("decision".to_string())),
            ("game", Json::Str(self.setup.game_id.clone())),
            ("ply", Json::Num(self.ply as f64)),
            ("round", Json::Num(round_ as f64)),
            ("age", Json::Str(age.to_string())),
            ("actor", Json::Str(format!("p{}", self.setup.seat))),
            ("state", Json::Str(state_text.to_string())),
            ("ranked", Json::Arr(ranked)),
            ("played", played.map(Json::Str).unwrap_or(Json::Null)),
            ("source", Json::Str(source.as_str().to_string())),
            ("latency_s", latency_s.map(Json::Num).unwrap_or(Json::Null)),
            ("note", Json::Str(note.to_string())),
        ]);
        self.write(rec)
    }

    /// What the human reported about the shared board and the rivals.
    /// `patches` keeps the literal lines typed -- when the mirror turns out
    /// to have drifted, these are the only forensic trail.
    pub fn observed(&mut self, round_: u16, dealt: &[usize], rivals: &[(u8, Vec<(String, mirror::Value)>)], patches: &[String]) -> Json {
        let rec = Json::obj(vec![
            ("v", Json::Num(SCHEMA as f64)),
            ("type", Json::Str("observed".to_string())),
            ("game", Json::Str(self.setup.game_id.clone())),
            ("round", Json::Num(round_ as f64)),
            ("dealt", Json::Arr(dealt.iter().map(|&d| Json::Num(d as f64)).collect())),
            (
                "rivals",
                Json::Obj(
                    rivals
                        .iter()
                        .map(|(idx, vals)| {
                            (idx.to_string(), Json::Obj(vals.iter().map(|(k, v)| (k.clone(), mirror::value_to_json(v))).collect()))
                        })
                        .collect(),
                ),
            ),
            ("patches", Json::Arr(patches.iter().map(|p| Json::Str(p.clone())).collect())),
        ]);
        self.write(rec)
    }

    pub fn check(&mut self, result: &mirror::CheckResult, state_text: Option<&str>) -> Json {
        let mut fields =
            vec![("v".to_string(), Json::Num(SCHEMA as f64)), ("type".to_string(), Json::Str("check".to_string())), ("game".to_string(), Json::Str(self.setup.game_id.clone()))];
        if let Json::Obj(rest) = result.to_json() {
            fields.extend(rest);
        }
        fields.push(("ok".to_string(), Json::Bool(!result.failed())));
        if let Some(s) = state_text {
            fields.push(("state".to_string(), Json::Str(s.to_string())));
        }
        self.write(Json::Obj(fields))
    }

    pub fn resync(&mut self, round_: u16, discrepancies: &[mirror::Discrepancy], patches: &[String], cause: &str) -> Json {
        self.resyncs.push(round_);
        let rec = Json::obj(vec![
            ("v", Json::Num(SCHEMA as f64)),
            ("type", Json::Str("resync".to_string())),
            ("game", Json::Str(self.setup.game_id.clone())),
            ("round", Json::Num(round_ as f64)),
            ("cause", Json::Str(cause.to_string())),
            ("patches", Json::Arr(patches.iter().map(|p| Json::Str(p.clone())).collect())),
            (
                "discrepancies",
                Json::Arr(
                    discrepancies
                        .iter()
                        .map(|d| {
                            Json::obj(vec![
                                ("key", Json::Str(d.key.clone())),
                                ("expected", mirror::value_to_json(&d.expected)),
                                ("reported", mirror::value_to_json(&d.reported)),
                                ("severity", Json::Str(d.severity.as_str().to_string())),
                                ("where", Json::Str(d.where_.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ]);
        self.write(rec)
    }

    /// The footer. `trusted` is the field aggregate analysis must filter on.
    /// A game that needed a resync had the bot choosing moves in a position
    /// that provably disagreed with the app for an unknown number of plies:
    /// we keep the data, but it is not admissible as a strength measurement
    /// without a human looking at the resync records first. Mirrors
    /// `result`.
    #[allow(clippy::too_many_arguments)]
    pub fn result(
        &mut self,
        scores: &[(String, i64)],
        rounds: u16,
        human_minutes: Option<i64>,
        notes: &str,
        aborted: bool,
        abort_reason: &str,
        effort: Json,
        observables: Vec<Json>,
    ) -> Json {
        let me = format!("p{}", self.setup.seat);
        let mut winner: Option<String> = None;
        let mut margin: Option<i64> = None;
        if !scores.is_empty() && !aborted {
            winner = scores.iter().max_by_key(|(_, v)| *v).map(|(k, _)| k.clone());
            let others_best = scores.iter().filter(|(k, _)| k != &me).map(|(_, v)| *v).max();
            if let (Some((_, my_score)), Some(best_other)) = (scores.iter().find(|(k, _)| k == &me), others_best) {
                margin = Some(my_score - best_other);
            }
        }
        let trusted = !aborted && self.resyncs.is_empty() && !scores.is_empty();
        let mut why = Vec::new();
        if aborted {
            why.push(if abort_reason.is_empty() { "aborted".to_string() } else { format!("aborted ({abort_reason})") });
        }
        if !self.resyncs.is_empty() {
            why.push(format!("mirror desynced and was resynced at round(s) {:?}", self.resyncs));
        }
        if scores.is_empty() {
            why.push("no final scores recorded".to_string());
        }
        let won = winner.as_ref().map(|w| *w == me);
        let rec = Json::obj(vec![
            ("v", Json::Num(SCHEMA as f64)),
            ("type", Json::Str("result".to_string())),
            ("game", Json::Str(self.setup.game_id.clone())),
            ("finished", Json::Str(now_iso8601())),
            ("scores", Json::Obj(scores.iter().map(|(k, v)| (k.clone(), Json::Num(*v as f64))).collect())),
            ("winner", winner.map(Json::Str).unwrap_or(Json::Null)),
            ("margin", margin.map(|m| Json::Num(m as f64)).unwrap_or(Json::Null)),
            ("won", won.map(Json::Bool).unwrap_or(Json::Null)),
            ("rounds", Json::Num(rounds as f64)),
            ("decisions", Json::Num(self.ply as f64)),
            ("human_minutes", human_minutes.map(|m| Json::Num(m as f64)).unwrap_or(Json::Null)),
            ("effort", effort),
            // The observable set as it stood at the END of the game -- it
            // can grow mid-game when the evaluator gains a feature.
            ("observables_final", Json::Arr(observables)),
            ("aborted", Json::Bool(aborted)),
            ("abort_reason", Json::Str(abort_reason.to_string())),
            ("resyncs", Json::Arr(self.resyncs.iter().map(|&r| Json::Num(r as f64)).collect())),
            ("trusted", Json::Bool(trusted)),
            ("untrusted_reason", Json::Str(why.join("; "))),
            ("limitations", Json::Arr(limitations(&self.setup))),
            ("notes", Json::Str(notes.to_string())),
        ]);
        let rec = self.write(rec);
        self.close();
        rec
    }

    pub fn close(&mut self) {
        self.file = None;
        self.closed = true;
    }
}

// ------------------------------------------------------------ reading back

pub fn load(path: &Path) -> Result<Vec<Json>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.is_empty() {
            out.push(parse_json(line).map_err(|e| e.to_string())?);
        }
    }
    Ok(out)
}

pub struct DroppedGame {
    pub path: PathBuf,
    pub why: String,
}

struct GameRow {
    players: i64,
    difficulty: String,
    mode: String,
    dlc: bool,
    personalities: Vec<String>,
    margin: Option<i64>,
    won: Option<bool>,
    human_minutes: Option<i64>,
    keystrokes: Option<i64>,
}

pub struct Arm {
    pub players: i64,
    pub difficulty: String,
    pub mode: String,
    pub dlc: bool,
    pub personalities: Vec<String>,
}

/// Aggregate over game logs. Untrusted games are counted, never pooled.
/// Mirrors `summarize`.
pub struct Summary {
    pub trusted_games: usize,
    pub dropped_games: usize,
    pub dropped: Vec<DroppedGame>,
    /// Games run under different setups are different experiments; pooling
    /// them is the quiet way to turn ten honest games into one dishonest
    /// number.
    pub arms: Vec<Arm>,
    pub poolable: bool,
    pub mean_margin: Option<f64>,
    pub sd_margin: Option<f64>,
    pub stderr_margin: Option<f64>,
    pub win_rate: Option<f64>,
    pub mean_human_minutes: Option<f64>,
    pub mean_keystrokes: Option<f64>,
    pub caveat: String,
}

fn json_i64(j: Option<&Json>) -> Option<i64> {
    j.and_then(Json::as_f64).map(|n| n as i64)
}

pub fn summarize(paths: &[PathBuf]) -> Summary {
    let mut games: Vec<GameRow> = Vec::new();
    let mut dropped: Vec<DroppedGame> = Vec::new();
    for p in paths {
        let recs = match load(p) {
            Ok(r) => r,
            Err(_) => {
                dropped.push(DroppedGame { path: p.clone(), why: "no result record".to_string() });
                continue;
            }
        };
        let head = recs.iter().find(|r| r.get("type").and_then(Json::as_str) == Some("game"));
        let res = recs.iter().find(|r| r.get("type").and_then(Json::as_str) == Some("result"));
        let (Some(head), Some(res)) = (head, res) else {
            dropped.push(DroppedGame { path: p.clone(), why: "no result record".to_string() });
            continue;
        };
        let setup = head.get("setup");
        let row = GameRow {
            players: json_i64(setup.and_then(|s| s.get("players"))).unwrap_or(0),
            difficulty: setup.and_then(|s| s.get("difficulty")).and_then(Json::as_str).unwrap_or("").to_string(),
            mode: setup.and_then(|s| s.get("mode")).and_then(Json::as_str).unwrap_or("").to_string(),
            dlc: setup.and_then(|s| s.get("dlc")).and_then(Json::as_bool).unwrap_or(false),
            personalities: setup
                .and_then(|s| s.get("personalities"))
                .and_then(Json::as_arr)
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default(),
            margin: json_i64(res.get("margin")),
            won: res.get("won").and_then(Json::as_bool),
            human_minutes: json_i64(res.get("human_minutes")),
            keystrokes: json_i64(res.get("effort").and_then(|e| e.get("keystrokes"))),
        };
        let trusted = res.get("trusted").and_then(Json::as_bool).unwrap_or(false);
        if trusted {
            games.push(row);
        } else {
            let why = res.get("untrusted_reason").and_then(Json::as_str).unwrap_or("").to_string();
            dropped.push(DroppedGame { path: p.clone(), why });
        }
    }

    let margins: Vec<i64> = games.iter().filter_map(|g| g.margin).collect();
    let wins: Vec<bool> = games.iter().filter_map(|g| g.won).collect();
    let mins: Vec<i64> = games.iter().filter_map(|g| g.human_minutes).filter(|&m| m != 0).collect();
    let keys: Vec<i64> = games.iter().filter_map(|g| g.keystrokes).filter(|&k| k != 0).collect();

    let n = margins.len();
    let mean = if n > 0 { Some(margins.iter().sum::<i64>() as f64 / n as f64) } else { None };
    let sd = if n > 1 {
        let m = mean.unwrap();
        Some((margins.iter().map(|&x| (x as f64 - m).powi(2)).sum::<f64>() / (n as f64 - 1.0)).sqrt())
    } else {
        None
    };

    let mut arm_keys: Vec<(i64, String, String, bool, Vec<String>)> =
        games.iter().map(|g| (g.players, g.difficulty.clone(), g.mode.clone(), g.dlc, g.personalities.clone())).collect();
    arm_keys.sort();
    arm_keys.dedup();
    let arms: Vec<Arm> = arm_keys
        .into_iter()
        .map(|(players, difficulty, mode, dlc, personalities)| Arm { players, difficulty, mode, dlc, personalities })
        .collect();
    let poolable = arms.len() <= 1;

    let mut caveat = "Win rate and margin here exclude the pact subsystem entirely; see each game's \
                       `limitations`."
        .to_string();
    if !poolable {
        caveat.push_str(&format!("  MIXED SETUPS: these games are NOT one experiment ({} distinct arms); do not report the pooled number.", arms.len()));
    }

    Summary {
        trusted_games: games.len(),
        dropped_games: dropped.len(),
        dropped,
        arms,
        poolable,
        mean_margin: mean,
        sd_margin: sd,
        stderr_margin: sd.map(|s| s / (n as f64).sqrt()),
        win_rate: if wins.is_empty() { None } else { Some(wins.iter().filter(|&&w| w).count() as f64 / wins.len() as f64) },
        mean_human_minutes: if mins.is_empty() { None } else { Some(mins.iter().sum::<i64>() as f64 / mins.len() as f64) },
        mean_keystrokes: if keys.is_empty() { None } else { Some(keys.iter().sum::<i64>() as f64 / keys.len() as f64) },
        caveat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Setup {
        Setup::new("t1")
    }

    // -------------------------------------------------------------- setup gates

    #[test]
    fn a_dlc_game_is_refused() {
        let mut s = setup();
        s.dlc = true;
        let err = s.validate().unwrap_err();
        assert!(err.contains("New Leaders"), "{err}");
    }

    #[test]
    fn an_unknown_difficulty_string_is_refused_at_parse_time() {
        // Unlike Python, an invalid `Difficulty` cannot be constructed at
        // all -- the runtime check Python's `validate()` performed moves to
        // the one place a string still becomes a `Difficulty`.
        assert!(Difficulty::parse("nightmare").is_err());
    }

    #[test]
    fn the_base_game_is_accepted() {
        assert!(setup().validate().is_ok());
    }

    #[test]
    fn a_seat_that_does_not_exist_is_refused() {
        let mut s = setup();
        s.players = 2;
        s.seat = 2;
        assert!(s.validate().is_err());
    }

    #[test]
    fn player_count_is_bounded_to_2_through_4() {
        let mut s = setup();
        s.players = 5;
        assert!(s.validate().is_err());
    }

    #[test]
    fn personalities_are_a_different_experiment_from_medium_difficulty() {
        let mut s = setup();
        s.difficulty = Difficulty::Medium;
        s.personalities = vec!["Napoleon".to_string()];
        assert!(s.validate().is_err());
    }

    #[test]
    fn setup_is_recorded_verbatim() {
        let mut s = setup();
        s.app_version = "2.4.1".to_string();
        let log = GameLog::new(s, None, &[]).unwrap();
        let head = &log.records[0];
        let setup_json = head.get("setup").unwrap();
        assert_eq!(setup_json.get("difficulty").and_then(Json::as_str), Some("hard"));
        assert_eq!(setup_json.get("app_version").and_then(Json::as_str), Some("2.4.1"));
        assert_eq!(setup_json.get("dlc").and_then(Json::as_bool), Some(false));
        assert_eq!(setup_json.get("edition").and_then(Json::as_str), Some("2015-base"));
    }

    // ---------------------------------------------------------------- pact bias

    #[test]
    fn the_header_carries_the_pact_note() {
        let log = GameLog::new(setup(), None, &[]).unwrap();
        let ids: Vec<&str> = log.records[0].get("limitations").and_then(Json::as_arr).unwrap().iter().filter_map(|l| l.get("id").and_then(Json::as_str)).collect();
        assert!(ids.contains(&"no_pacts"));
    }

    #[test]
    fn the_result_repeats_the_pact_note() {
        let mut log = GameLog::new(setup(), None, &[]).unwrap();
        let scores = vec![("p0".to_string(), 180), ("p1".to_string(), 200), ("p2".to_string(), 150)];
        let rec = log.result(&scores, 20, None, "", false, "", Json::Obj(vec![]), vec![]);
        let note = rec.get("limitations").and_then(Json::as_arr).unwrap().iter().find(|l| l.get("id").and_then(Json::as_str) == Some("no_pacts")).unwrap();
        assert_eq!(note.get("severity").and_then(Json::as_str), Some("high"));
        assert!(note.get("text").and_then(Json::as_str).unwrap().contains("STRICTLY SMALLER GAME"));
    }

    #[test]
    fn two_player_games_downgrade_the_pact_note_honestly() {
        let mut s = setup();
        s.players = 2;
        let log = GameLog::new(s, None, &[]).unwrap();
        let note = log.records[0].get("limitations").and_then(Json::as_arr).unwrap().iter().find(|l| l.get("id").and_then(Json::as_str) == Some("no_pacts")).unwrap();
        assert_eq!(note.get("severity").and_then(Json::as_str), Some("low"));
    }

    #[test]
    fn free_mode_says_the_score_is_not_the_bot() {
        let mut s = setup();
        s.mode = Mode::Free;
        let log = GameLog::new(s, None, &[]).unwrap();
        let ids: Vec<&str> = log.records[0].get("limitations").and_then(Json::as_arr).unwrap().iter().filter_map(|l| l.get("id").and_then(Json::as_str)).collect();
        assert!(ids.contains(&"free_mode"));
    }

    // ------------------------------------------------------------------- trust

    #[test]
    fn a_clean_game_is_trusted() {
        let mut log = GameLog::new(setup(), None, &[]).unwrap();
        let scores = vec![("p0".to_string(), 200), ("p1".to_string(), 180), ("p2".to_string(), 150)];
        let rec = log.result(&scores, 20, None, "", false, "", Json::Obj(vec![]), vec![]);
        assert_eq!(rec.get("trusted").and_then(Json::as_bool), Some(true));
        assert_eq!(rec.get("won").and_then(Json::as_bool), Some(true));
        assert_eq!(rec.get("margin").and_then(Json::as_f64), Some(20.0));
    }

    #[test]
    fn a_resynced_game_is_not_trusted() {
        let mut log = GameLog::new(setup(), None, &[]).unwrap();
        log.resync(7, &[], &["p1 c=40".to_string()], "misread the culture track");
        let scores = vec![("p0".to_string(), 200), ("p1".to_string(), 180), ("p2".to_string(), 150)];
        let rec = log.result(&scores, 20, None, "", false, "", Json::Obj(vec![]), vec![]);
        assert_eq!(rec.get("trusted").and_then(Json::as_bool), Some(false));
        assert!(rec.get("untrusted_reason").and_then(Json::as_str).unwrap().contains('7'));
    }

    #[test]
    fn an_aborted_game_has_no_winner() {
        let mut log = GameLog::new(setup(), None, &[]).unwrap();
        let rec = log.result(&[], 9, None, "", true, "desync", Json::Obj(vec![]), vec![]);
        assert_eq!(rec.get("trusted").and_then(Json::as_bool), Some(false));
        assert_eq!(rec.get("winner"), Some(&Json::Null));
    }

    #[test]
    fn margin_is_against_the_best_opponent() {
        let mut log = GameLog::new(setup(), None, &[]).unwrap();
        let scores = vec![("p0".to_string(), 150), ("p1".to_string(), 180), ("p2".to_string(), 200)];
        let rec = log.result(&scores, 20, None, "", false, "", Json::Obj(vec![]), vec![]);
        assert_eq!(rec.get("margin").and_then(Json::as_f64), Some(-50.0));
        assert_eq!(rec.get("won").and_then(Json::as_bool), Some(false));
    }

    // ---------------------------------------------------------------- round trip

    /// A game abandoned in round 9 must still leave rounds 1-9 on disk.
    #[test]
    fn jsonl_is_flushed_per_record() {
        let dir = std::env::temp_dir().join(format!("harness-record-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("g.jsonl");
        let mut log = GameLog::new(setup(), Some(&path), &[]).unwrap();
        log.decision("tta 1\n...", vec![], Some("[\"take\", 3]".to_string()), Source::Bot, 4, "I", None, "");
        // deliberately do NOT close
        let recs = load(&path).unwrap();
        let types: Vec<&str> = recs.iter().filter_map(|r| r.get("type").and_then(Json::as_str)).collect();
        assert_eq!(types, vec!["game", "decision"]);
        log.close();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_record_round_trips_through_json() {
        let dir = std::env::temp_dir().join(format!("harness-record-test2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("g.jsonl");
        let mut log = GameLog::new(setup(), Some(&path), &[]).unwrap();
        log.decision(
            "snap",
            vec![Json::obj(vec![("move", Json::Str("take 1".to_string())), ("score", Json::Num(1.5))])],
            Some("take 1".to_string()),
            Source::Bot,
            3,
            "I",
            Some(2.0),
            "",
        );
        log.observed(3, &[10, 11], &[(1, vec![("c".to_string(), mirror::Value::Int(20))])], &["take p1 4".to_string()]);
        let scores = vec![("p0".to_string(), 1), ("p1".to_string(), 2), ("p2".to_string(), 3)];
        log.result(&scores, 18, Some(64), "", false, "", Json::Obj(vec![]), vec![]);
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines() {
            parse_json(line).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Section 6b: embed the replayable snapshot verbatim so every logged
    /// position can be replayed by a future bot.
    #[test]
    fn the_state_snapshot_is_stored_as_a_string() {
        let mut log = GameLog::new(setup(), None, &[]).unwrap();
        let rec = log.decision("tta 1\ngame 3p ...", vec![], Some("end_turn".to_string()), Source::Bot, 5, "I", None, "");
        assert!(matches!(rec.get("state"), Some(Json::Str(_))));
    }

    // -------------------------------------------------------------------- aggregate

    fn write_game(dir: &Path, name: &str, scores: &[(&str, i64)], setup_kw: impl FnOnce(&mut Setup)) -> PathBuf {
        let path = dir.join(name);
        let mut s = setup();
        s.game_id = name.to_string();
        setup_kw(&mut s);
        let mut log = GameLog::new(s, Some(&path), &[]).unwrap();
        let scores: Vec<(String, i64)> = scores.iter().map(|(k, v)| (k.to_string(), *v)).collect();
        log.result(&scores, 20, None, "", false, "", Json::Obj(vec![]), vec![]);
        path
    }

    #[test]
    fn untrusted_games_are_counted_but_never_pooled() {
        let dir = std::env::temp_dir().join(format!("harness-record-agg1-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = write_game(&dir, "a.jsonl", &[("p0", 200), ("p1", 180), ("p2", 150)], |_| {});
        let path = dir.join("b.jsonl");
        let mut log = GameLog::new({ let mut s = setup(); s.game_id = "b".to_string(); s }, Some(&path), &[]).unwrap();
        log.resync(4, &[], &[], "unknown");
        log.result(&[("p0".to_string(), 100), ("p1".to_string(), 300), ("p2".to_string(), 150)], 20, None, "", false, "", Json::Obj(vec![]), vec![]);
        let s = summarize(&[good, path]);
        assert_eq!(s.trusted_games, 1);
        assert_eq!(s.dropped_games, 1);
        assert_eq!(s.mean_margin, Some(20.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mixed_setups_are_not_poolable() {
        let dir = std::env::temp_dir().join(format!("harness-record-agg2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = write_game(&dir, "a.jsonl", &[("p0", 200), ("p1", 180), ("p2", 150)], |_| {});
        let b = write_game(&dir, "b.jsonl", &[("p0", 200), ("p1", 180), ("p2", 150)], |s| s.difficulty = Difficulty::Medium);
        let s = summarize(&[a, b]);
        assert!(!s.poolable);
        assert!(s.caveat.contains("MIXED SETUPS"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_same_setup_twice_is_poolable() {
        let dir = std::env::temp_dir().join(format!("harness-record-agg3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = write_game(&dir, "a.jsonl", &[("p0", 200), ("p1", 180), ("p2", 150)], |_| {});
        let b = write_game(&dir, "b.jsonl", &[("p0", 100), ("p1", 180), ("p2", 150)], |_| {});
        let s = summarize(&[a, b]);
        assert!(s.poolable);
        assert_eq!(s.win_rate, Some(0.5));
        assert_eq!(s.mean_margin, Some(-30.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_caveat_always_mentions_pacts() {
        let dir = std::env::temp_dir().join(format!("harness-record-agg4-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = write_game(&dir, "a.jsonl", &[("p0", 200), ("p1", 1), ("p2", 1)], |_| {});
        let s = summarize(&[a]);
        assert!(s.caveat.contains("pact"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_log_without_a_result_is_dropped_not_ignored() {
        let dir = std::env::temp_dir().join(format!("harness-record-agg5-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.jsonl");
        let mut log = GameLog::new(setup(), Some(&path), &[]).unwrap();
        log.decision("s", vec![], Some("end_turn".to_string()), Source::Bot, 1, "I", None, "");
        log.close();
        let s = summarize(&[path]);
        assert_eq!(s.dropped_games, 1);
        assert_eq!(s.dropped[0].why, "no result record");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
