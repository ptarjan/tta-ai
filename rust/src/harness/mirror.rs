//! Desync detection: make a drifting mirror fail loudly, not silently.
//!
//! A mirror that has quietly diverged from the app is not a degraded
//! measurement, it is a FABRICATED one -- the bot was asked to move in a
//! position that never existed. The asymmetry that makes this tractable:
//!
//! * **Our own board is SIMULATED.** Every one of our moves goes through the
//!   real engine, so the mirror *predicts* our culture, science, strength,
//!   food, resources, actions, hand sizes and banks. A prediction can be
//!   checked, and the app prints all of it on one panel. These are the real
//!   checksums ([`SelfCheckKey`]/[`check_self`]).
//! * **Rival boards are FORCED.** We never replay a rival's turn; the human
//!   types a handful of numbers off the app's player panel and
//!   `state_io::patch` back-solves the mirror to match. A forced value can
//!   never disagree with itself, so it is *not* a checksum. The only
//!   cross-check available there is arithmetic consistency over time
//!   ([`RivalHistory`]), which is a warning, not a proof.
//!
//! One exception: completed wonders are DERIVED, not forced -- their names
//! arrive as they happen (`p1 built+ Colossus`), so the mirror holds an
//! opinion about how many each rival has, and an opinion can be checked
//! against the number on the panel ([`RIVAL_CHECKS`], the only hard rival
//! check here). Colonies and a wonder-in-progress are the same shape.
//!
//! So: [`SELF_CHECKS`]/[`RIVAL_CHECKS`] disagreeing is always [`Severity::
//! Fail`], and [`RivalHistory`] disagreeing is always [`Severity::Warn`].
//! Nothing here ever "fixes up" the mirror on its own -- an automatic repair
//! is exactly how a silent corruption survives.
//!
//! Ported from `harness/mirror.py`. Builds on `advisor::state_io`, which
//! already ports the snapshot format and `patch` this module reads and
//! writes through (see that module's own doc comment; commit `8ccbfb7`).

use std::collections::HashMap;
use std::fmt;

use crate::advisor::state_io::{age_str, Board};
use crate::economy;
use crate::effects;
use crate::harness::fields::ProbeId;
use crate::state::{GameState, PlayerState};

// -------------------------------------------------------------- severity

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// The mirror is provably wrong. Stop.
    Fail,
    /// Suspicious, not proof. Show it, let the operator judge.
    Warn,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Fail => "fail",
            Severity::Warn => "warn",
        }
    }
}

/// A checked or reported quantity: every field here is either a whole number
/// off a panel or a short code (`age=II`). Mirrors Python's `int | str`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Int(i64),
    Text(String),
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Text(s) => write!(f, "{s}"),
        }
    }
}

// ------------------------------------------------------------- self checks

/// One quantity checked against our OWN board. Mirrors `SELF_CHECKS`' keys;
/// the first five are the fast positional "spine" ([`SPINE`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelfCheckKey {
    C,
    S,
    Str,
    F,
    R,
    Cr,
    Sr,
    Hap,
    Ca,
    Ma,
    Fw,
    Yel,
    Blue,
    Hc,
    Hm,
}

pub const SELF_CHECKS: &[SelfCheckKey] = &[
    SelfCheckKey::C,
    SelfCheckKey::S,
    SelfCheckKey::Str,
    SelfCheckKey::F,
    SelfCheckKey::R,
    SelfCheckKey::Cr,
    SelfCheckKey::Sr,
    SelfCheckKey::Hap,
    SelfCheckKey::Ca,
    SelfCheckKey::Ma,
    SelfCheckKey::Fw,
    SelfCheckKey::Yel,
    SelfCheckKey::Blue,
    SelfCheckKey::Hc,
    SelfCheckKey::Hm,
];

/// The fast positional form, in the order a spine line's slashes fill:
/// `41/12/9/3/5` = `c/s/str/f/r`.
pub const SPINE: &[SelfCheckKey] = &[SelfCheckKey::C, SelfCheckKey::S, SelfCheckKey::Str, SelfCheckKey::F, SelfCheckKey::R];

impl SelfCheckKey {
    pub fn as_str(self) -> &'static str {
        use SelfCheckKey::*;
        match self {
            C => "c",
            S => "s",
            Str => "str",
            F => "f",
            R => "r",
            Cr => "cr",
            Sr => "sr",
            Hap => "hap",
            Ca => "ca",
            Ma => "ma",
            Fw => "fw",
            Yel => "yel",
            Blue => "blue",
            Hc => "hc",
            Hm => "hm",
        }
    }

    pub fn label(self) -> &'static str {
        use SelfCheckKey::*;
        match self {
            C => "culture (your score)",
            S => "science",
            Str => "military strength",
            F => "food in store",
            R => "resources in store",
            Cr => "culture per turn",
            Sr => "science per turn",
            Hap => "happy faces",
            Ca => "civil actions available (the total, not what is left)",
            Ma => "military actions available",
            Fw => "unused (yellow) workers",
            Yel => "yellow bank",
            Blue => "blue bank tokens",
            Hc => "civil cards in your hand",
            Hm => "military cards in your hand",
        }
    }

    /// What the mirror predicts the app is showing for this key. No
    /// `effects::invalidate` -- this port's `effects::compute` recomputes
    /// fresh every call (see `advisor::state_io`'s own doc comment on why).
    fn get(self, state: &GameState, p: &PlayerState) -> i64 {
        use SelfCheckKey::*;
        match self {
            C => p.culture as i64,
            S => p.science as i64,
            F => p.food as i64,
            R => p.resources as i64,
            Fw => p.workers_free as i64,
            Yel => p.yellow_bank as i64,
            Blue => economy::blue_available(p) as i64,
            Hc => p.hand_civil.len() as i64,
            Hm => p.hand_military.len() as i64,
            Str => effects::compute(state, p).strength as i64,
            Cr => effects::compute(state, p).culture as i64,
            Sr => effects::compute(state, p).science as i64,
            Hap => effects::compute(state, p).happy as i64,
            Ca => effects::compute(state, p).civil_actions as i64,
            Ma => effects::compute(state, p).military_actions as i64,
        }
    }
}

/// Board-wide checks: cheap, and they catch the worst class of drift (the
/// mirror sitting in a different round or age from the app, which
/// invalidates every horizon-scaled weight at once).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BoardKey {
    Round,
    Age,
    Row,
}

pub const BOARD_CHECKS: &[BoardKey] = &[BoardKey::Round, BoardKey::Age, BoardKey::Row];

impl BoardKey {
    pub fn as_str(self) -> &'static str {
        match self {
            BoardKey::Round => "round",
            BoardKey::Age => "age",
            BoardKey::Row => "row",
        }
    }

}

// ------------------------------------------------------------ rival checks

/// What the operator reads off one rival's panel, in prompt order. All of it
/// is public information in Through the Ages (RULES_SPEC.md 2.6). ORDER IS
/// API: the positional form `p1 41/5/3/12/4/3/1` zips against this list.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RivalAskKey {
    C,
    Cr,
    Sr,
    Str,
    Ca,
    Hc,
    W,
    S,
    F,
    R,
    Fw,
    Y,
    Ma,
    Col,
    Wip,
}

pub const RIVAL_ASK_KEYS: &[RivalAskKey] = &[
    RivalAskKey::C,
    RivalAskKey::Cr,
    RivalAskKey::Sr,
    RivalAskKey::Str,
    RivalAskKey::Ca,
    RivalAskKey::Hc,
    RivalAskKey::W,
    RivalAskKey::S,
    RivalAskKey::F,
    RivalAskKey::R,
    RivalAskKey::Fw,
    RivalAskKey::Y,
    RivalAskKey::Ma,
    RivalAskKey::Col,
    RivalAskKey::Wip,
];

/// Pushed into the mirror as `state_io::patch` lines: `c/cr/sr/str`
/// back-solve through the forced fields, `ca`/`ma` set the action counters,
/// `hc` sets the rival's total hand size, `s/f/r/fw/y` are plain scalars.
pub const RIVAL_FORCE_KEYS: &[RivalAskKey] = &[
    RivalAskKey::C,
    RivalAskKey::Cr,
    RivalAskKey::Sr,
    RivalAskKey::Str,
    RivalAskKey::Ca,
    RivalAskKey::Hc,
    RivalAskKey::S,
    RivalAskKey::F,
    RivalAskKey::R,
    RivalAskKey::Fw,
    RivalAskKey::Y,
    RivalAskKey::Ma,
];

/// CHECKED instead of forced, because a NAME carries effects a count cannot:
/// the mirror learns wonders/colonies/a wonder-in-progress by name as they
/// happen, and these three counts verify none was missed. The only hard
/// checks on the rival side -- a forced value cannot disagree with itself.
pub const RIVAL_CHECKS: &[RivalAskKey] = &[RivalAskKey::W, RivalAskKey::Col, RivalAskKey::Wip];

impl RivalAskKey {
    pub fn as_str(self) -> &'static str {
        use RivalAskKey::*;
        match self {
            C => "c",
            Cr => "cr",
            Sr => "sr",
            Str => "str",
            Ca => "ca",
            Hc => "hc",
            W => "w",
            S => "s",
            F => "f",
            R => "r",
            Fw => "fw",
            Y => "y",
            Ma => "ma",
            Col => "col",
            Wip => "wip",
        }
    }

    /// One line of operator-facing help per asked field.
    pub fn label(self) -> &'static str {
        use RivalAskKey::*;
        match self {
            C => "culture (their score)",
            Cr => "culture per turn",
            Sr => "science per turn",
            Str => "military strength",
            Ca => "civil actions (their total -- between turns it is all of them)",
            Hc => "civil cards in hand",
            W => "completed wonders",
            S => "science stock (the number, not the rate)",
            F => "food stock",
            R => "resource stock",
            Fw => "unused (yellow) workers",
            Y => "yellow tokens still in their bank",
            Ma => "military actions (their total)",
            Col => "colonies taken",
            Wip => "1 if they are part-way through a wonder, else 0",
        }
    }

    /// This key's probe in `harness::fields`, so the static ask here and the
    /// derived field list can be held against each other.
    pub fn probe_id(self) -> ProbeId {
        use RivalAskKey::*;
        match self {
            C => ProbeId::RivalCulture,
            Cr => ProbeId::RivalCultureRate,
            Sr => ProbeId::RivalScienceRate,
            Str => ProbeId::RivalStrength,
            Ca => ProbeId::RivalCivilActions,
            Hc => ProbeId::RivalHandCivilSize,
            W => ProbeId::RivalWonders,
            S => ProbeId::RivalScience,
            F => ProbeId::RivalFood,
            R => ProbeId::RivalResources,
            Fw => ProbeId::RivalWorkersFree,
            Y => ProbeId::RivalYellowBank,
            Ma => ProbeId::RivalMilitaryActions,
            Col => ProbeId::RivalColonies,
            Wip => ProbeId::RivalWonderProgress,
        }
    }

    /// What the mirror believes about this CHECKED quantity. Only meaningful
    /// for a key in [`RIVAL_CHECKS`].
    fn get_checked(self, q: &PlayerState) -> i64 {
        match self {
            RivalAskKey::W => q.completed_wonders.len() as i64,
            RivalAskKey::Col => q.colonies.len() as i64,
            RivalAskKey::Wip => {
                if q.wonder.is_none() {
                    0
                } else {
                    1
                }
            }
            RivalAskKey::C | RivalAskKey::Cr | RivalAskKey::Sr | RivalAskKey::Str | RivalAskKey::Ca | RivalAskKey::Hc | RivalAskKey::S | RivalAskKey::F | RivalAskKey::R | RivalAskKey::Fw | RivalAskKey::Y | RivalAskKey::Ma => unreachable!("get_checked called on a non-RIVAL_CHECKS key: {self:?}"),
        }
    }
}

// ---------------------------------------------------------- discrepancies

#[derive(Clone, Debug, PartialEq)]
pub struct Discrepancy {
    pub key: String,
    pub expected: Value,
    pub reported: Value,
    pub severity: Severity,
    pub where_: String,
}

impl fmt::Display for Discrepancy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.where_.is_empty() {
            write!(f, "{} ", self.where_)?;
        }
        write!(f, "{}: mirror says {}, you read {}", self.key, self.expected, self.reported)
    }
}

// ------------------------------------------------------------- snapshots

/// What the mirror predicts the app is showing on our own panel. Mirrors
/// `self_snapshot`.
pub fn self_snapshot(board: &Board, keys: &[SelfCheckKey]) -> Vec<(String, Value)> {
    let p = &board.state.players[board.me as usize];
    keys.iter().map(|&k| (k.as_str().to_string(), Value::Int(k.get(&board.state, p)))).collect()
}

/// Mirrors `board_snapshot`.
pub fn board_snapshot(board: &Board) -> Vec<(String, Value)> {
    let st = &board.state;
    let row = st.card_row.iter().filter(|c| !c.is_none()).count() as i64;
    vec![
        (BoardKey::Round.as_str().to_string(), Value::Int(st.round as i64)),
        (BoardKey::Age.as_str().to_string(), Value::Text(age_str(st.age_civil).to_string())),
        (BoardKey::Row.as_str().to_string(), Value::Int(row)),
    ]
}

/// What the mirror believes about the CHECKED rival quantities. Mirrors
/// `rival_snapshot`.
pub fn rival_snapshot(state: &GameState, idx: u8) -> Vec<(String, Value)> {
    let q = &state.players[idx as usize];
    RIVAL_CHECKS.iter().map(|&k| (k.as_str().to_string(), Value::Int(k.get_checked(q)))).collect()
}

/// Every key present in BOTH `expected` and `reported` that disagrees.
/// Absent keys are not silently passed as OK -- they are simply not checked.
/// Mirrors `compare`.
pub fn compare(expected: &[(String, Value)], reported: &[(String, Value)], severity: Severity, where_: &str) -> Vec<Discrepancy> {
    let mut out = Vec::new();
    for (k, want) in expected {
        let Some((_, got)) = reported.iter().find(|(rk, _)| rk == k) else { continue };
        let same = match (want, got) {
            (Value::Int(a), Value::Int(b)) => a == b,
            _ => want.to_string().trim().to_uppercase() == got.to_string().trim().to_uppercase(),
        };
        if !same {
            out.push(Discrepancy { key: k.clone(), expected: want.clone(), reported: got.clone(), severity, where_: where_.to_string() });
        }
    }
    out
}

pub fn check_self(board: &Board, reported: &[(String, Value)]) -> Vec<Discrepancy> {
    compare(&self_snapshot(board, SELF_CHECKS), reported, Severity::Fail, &format!("p{}", board.me))
}

pub fn check_board(board: &Board, reported: &[(String, Value)]) -> Vec<Discrepancy> {
    compare(&board_snapshot(board), reported, Severity::Fail, "board")
}

/// Hard checks on rival values the mirror DERIVES rather than forces.
/// `rivals` is `{idx: {key: value}}`. Mirrors `check_rivals`.
pub fn check_rivals(state: &GameState, rivals: &[(u8, Vec<(String, Value)>)]) -> Vec<Discrepancy> {
    let mut out = Vec::new();
    for (idx, vals) in rivals {
        if (*idx as usize) >= state.num_players as usize {
            continue;
        }
        out.extend(compare(&rival_snapshot(state, *idx), vals, Severity::Fail, &format!("p{idx}")));
    }
    out
}

// ------------------------------------------------------ rival consistency

/// Arithmetic consistency for values we can only ever be TOLD. We force a
/// rival's culture, so the mirror cannot contradict it -- but we were also
/// told their culture RATE last round, and culture only moves by production
/// plus card/event effects. A report that violates `c_now >= c_prev +
/// rate_prev - slack` is usually a misread panel or a transposed digit,
/// caught for free. Mirrors `RivalHistory`.
pub struct RivalHistory {
    pub slack: i64,
    seen: HashMap<u8, (u16, i64, Option<i64>)>,
}

impl RivalHistory {
    pub fn new() -> RivalHistory {
        RivalHistory { slack: 8, seen: HashMap::new() }
    }

    pub fn check(&mut self, idx: u8, rnd: u16, culture: Option<i64>, rate: Option<i64>) -> Vec<Discrepancy> {
        let mut out = Vec::new();
        if let Some(&(p_rnd, p_c, p_rate)) = self.seen.get(&idx) {
            if let (Some(culture), Some(p_rate)) = (culture, p_rate) {
                let turns = rnd.saturating_sub(p_rnd) as i64;
                if turns > 0 {
                    let lo = p_c + p_rate * turns - self.slack;
                    let hi = p_c + p_rate * turns + 6 * turns + self.slack;
                    if !(lo..=hi).contains(&culture) {
                        out.push(Discrepancy {
                            key: "culture".to_string(),
                            expected: Value::Text(format!("{lo}..{hi} (from +{p_rate}/turn at round {p_rnd})")),
                            reported: Value::Int(culture),
                            severity: Severity::Warn,
                            where_: format!("p{idx}"),
                        });
                    }
                }
            }
        }
        if let Some(culture) = culture {
            self.seen.insert(idx, (rnd, culture, rate));
        }
        out
    }
}

impl Default for RivalHistory {
    fn default() -> RivalHistory {
        RivalHistory::new()
    }
}

// ------------------------------------------------------------- the parser

/// Parse a check line. Two forms, both accepted on one line: the fast
/// positional spine (`41/12/9/3/5`, in `spine` order) or explicit keys, any
/// order, any subset (`c=41 s=12 str=9 age=II`). A key not in `allowed` is
/// an error, never a silently dropped field. Mirrors `parse_line`.
pub fn parse_line(text: &str, spine: &[&str], allowed: &[&str]) -> (Vec<(String, Value)>, Vec<String>) {
    let mut vals: Vec<(String, Value)> = Vec::new();
    let mut errs: Vec<String> = Vec::new();
    let set = |vals: &mut Vec<(String, Value)>, key: &str, v: Value| {
        if let Some(entry) = vals.iter_mut().find(|(k, _)| k == key) {
            entry.1 = v;
        } else {
            vals.push((key.to_string(), v));
        }
    };
    for tok in text.replace(',', " ").split_whitespace() {
        if let Some(eq) = tok.find('=') {
            let k = tok[..eq].trim().to_lowercase();
            // accept "ca=3/4": only the part before the first '/' is the value
            let v = tok[eq + 1..].trim().split('/').next().unwrap_or("").to_string();
            if !allowed.contains(&k.as_str()) {
                errs.push(format!("unknown check field {k:?}"));
                continue;
            }
            if k == "age" {
                set(&mut vals, &k, Value::Text(v.to_uppercase()));
                continue;
            }
            match v.parse::<i64>() {
                Ok(n) => set(&mut vals, &k, Value::Int(n)),
                Err(_) => errs.push(format!("{k}: {v:?} is not a number")),
            }
            continue;
        }
        if tok.contains('/') {
            let parts: Vec<&str> = tok.split('/').collect();
            if parts.len() > spine.len() {
                errs.push(format!("the spine is {} ({} numbers), you gave {}", spine.join("/"), spine.len(), parts.len()));
                continue;
            }
            for (&key, part) in spine.iter().zip(parts.iter()) {
                let part = part.trim();
                if part.is_empty() || part == "?" {
                    continue;
                }
                match part.parse::<i64>() {
                    Ok(n) => set(&mut vals, key, Value::Int(n)),
                    Err(_) => errs.push(format!("{key}: {part:?} is not a number")),
                }
            }
            continue;
        }
        errs.push(format!("cannot read {tok:?} -- use 41/12/9/3/5 or c=41"));
    }
    (vals, errs)
}

fn self_and_board_keys() -> Vec<&'static str> {
    SELF_CHECKS.iter().map(|k| k.as_str()).chain(BOARD_CHECKS.iter().map(|k| k.as_str())).collect()
}

fn spine_strs(spine: &[SelfCheckKey]) -> Vec<&'static str> {
    spine.iter().map(|k| k.as_str()).collect()
}

/// `parse_line` against our own board's spine/allowed set -- what
/// `harness::play` actually calls at the "you ..." prompt.
pub fn parse_self_line(text: &str) -> (Vec<(String, Value)>, Vec<String>) {
    parse_line(text, &spine_strs(SPINE), &self_and_board_keys())
}

fn rival_ask_strs() -> Vec<&'static str> {
    RIVAL_ASK_KEYS.iter().map(|k| k.as_str()).collect()
}

/// `p1 c=41 cr=5 sr=3 str=12` -> `(idx, {key: value}, errors)`. Accepts the
/// bare positional form too, and any PREFIX of it -- a trailing field left
/// off is simply not reported. Mirrors `parse_rival_line`.
pub fn parse_rival_line(text: &str) -> (Option<u8>, Vec<(String, Value)>, Vec<String>) {
    let toks: Vec<&str> = text.split_whitespace().collect();
    let Some((&first, rest)) = toks.split_first() else {
        return (None, Vec::new(), vec!["empty".to_string()]);
    };
    let who = first.to_lowercase();
    let digits = who.trim_start_matches('p');
    let idx = match digits.parse::<u8>() {
        Ok(n) => n,
        Err(_) => return (None, Vec::new(), vec![format!("{first:?} is not a player like 'p1'")]),
    };
    let keys = rival_ask_strs();
    let (vals, errs) = parse_line(&rest.join(" "), &keys, &keys);
    (Some(idx), vals, errs)
}

/// Spine keys the operator did not supply. A round is not verified without
/// them; `harness::play` refuses to move on. Mirrors `missing_spine`.
pub fn missing_spine(reported: &[(String, Value)], spine: &[SelfCheckKey]) -> Vec<String> {
    spine.iter().filter(|k| !reported.iter().any(|(rk, _)| rk == k.as_str())).map(|k| k.as_str().to_string()).collect()
}

// -------------------------------------------------------------- the verdict

pub struct CheckResult {
    pub round: u16,
    pub reported: Vec<(String, Value)>,
    pub discrepancies: Vec<Discrepancy>,
}

/// One rival row of `CheckResult::read_the_panel`'s output: `(idx, forced
/// numbers, name channels)`. `read_the_panel` itself is only called from
/// `#[cfg(test)]` below, which the plain (non-test) `lib` build doesn't see
/// -- so, unlike the function, this newly-named alias needs its own
/// `#[allow(dead_code)]` to stay quiet in that build too.
#[allow(dead_code)]
type PanelRowLocal = (u8, Vec<(String, Value)>, Vec<(&'static str, Vec<String>)>);

impl CheckResult {
    pub fn failed(&self) -> bool {
        self.discrepancies.iter().any(|d| d.severity == Severity::Fail)
    }

    pub fn warned(&self) -> bool {
        self.discrepancies.iter().any(|d| d.severity == Severity::Warn)
    }

    pub fn to_json(&self) -> crate::fixtures::Json {
        use crate::fixtures::Json;
        Json::obj(vec![
            ("round", Json::Num(self.round as f64)),
            ("reported", Json::Obj(self.reported.iter().map(|(k, v)| (k.clone(), value_to_json(v))).collect())),
            (
                "discrepancies",
                Json::Arr(
                    self.discrepancies
                        .iter()
                        .map(|d| {
                            Json::obj(vec![
                                ("key", Json::Str(d.key.clone())),
                                ("expected", value_to_json(&d.expected)),
                                ("reported", value_to_json(&d.reported)),
                                ("severity", Json::Str(d.severity.as_str().to_string())),
                                ("where", Json::Str(d.where_.clone())),
                            ])
                        })
                        .collect(),
                ),
            ),
        ])
    }
}

pub(crate) fn value_to_json(v: &Value) -> crate::fixtures::Json {
    use crate::fixtures::Json;
    match v {
        Value::Int(n) => Json::Num(*n as f64),
        Value::Text(s) => Json::Str(s.clone()),
    }
}

fn get_int(vals: &[(String, Value)], key: &str) -> Option<i64> {
    vals.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
        Value::Int(n) => Some(*n),
        Value::Text(_) => None,
    })
}

/// Full per-round verification. `rivals` is `{idx: {key: value}}`. Called
/// BEFORE the forced rival values are applied, which is the only order that
/// makes `check_rivals` mean anything: after the patches the mirror agrees
/// with the operator by construction. Mirrors `round_check`.
pub fn round_check(
    board: &Board,
    reported: &[(String, Value)],
    history: Option<&mut RivalHistory>,
    rivals: &[(u8, Vec<(String, Value)>)],
) -> CheckResult {
    let mut ds = check_self(board, reported);
    ds.extend(check_board(board, reported));
    ds.extend(check_rivals(&board.state, rivals));
    if let Some(hist) = history {
        for (idx, vals) in rivals {
            ds.extend(hist.check(*idx, board.state.round, get_int(vals, "c"), get_int(vals, "cr")));
        }
    }
    CheckResult { round: board.state.round, reported: reported.to_vec(), discrepancies: ds }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::advisor::state_io as S;
    use crate::bots::weighted::{features, weights::WeightKey};
    use crate::harness::fields as F;
    use crate::harness::test_support::midgame;

    // ------------------------------------------------------- self checks

    #[test]
    fn a_fresh_snapshot_matches_itself() {
        let board = midgame(3, 0, 5, 8);
        let snap = self_snapshot(&board, SELF_CHECKS);
        assert_eq!(check_self(&board, &snap), Vec::new());
    }

    /// Perturb each checked quantity; the check must catch each one. A check
    /// that cannot fail is decoration.
    #[test]
    fn every_self_field_is_a_real_tripwire() {
        let board = midgame(3, 0, 5, 8);
        let snap = self_snapshot(&board, SELF_CHECKS);
        for (key, val) in &snap {
            let Value::Int(n) = val else { continue };
            let mut bad = snap.clone();
            for (k, v) in bad.iter_mut() {
                if k == key {
                    *v = Value::Int(n + 7);
                }
            }
            let ds = check_self(&board, &bad);
            assert!(ds.iter().any(|d| &d.key == key), "{key} drifted by 7 and nothing noticed");
            assert!(ds.iter().all(|d| d.severity == Severity::Fail));
        }
    }

    #[test]
    fn board_checks_catch_wrong_round_age_and_row() {
        let board = midgame(3, 0, 5, 8);
        let snap = board_snapshot(&board);
        assert_eq!(check_board(&board, &snap), Vec::new());
        for key in ["round", "age", "row"] {
            let mut bad = snap.clone();
            for (k, v) in bad.iter_mut() {
                if k == key {
                    *v = match v {
                        Value::Int(n) => Value::Int(*n + 1),
                        Value::Text(s) => Value::Text(if s == "III" { "I".to_string() } else { "III".to_string() }),
                    };
                }
            }
            let d = check_board(&board, &bad);
            assert_eq!(d.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), vec![key]);
        }
    }

    /// Simulate the classic failure: an event we forgot to enter. The app
    /// gives us 6 culture, the mirror does not hear about it, and the next
    /// round's check must fail.
    #[test]
    fn a_real_drift_is_caught() {
        let board = midgame(3, 0, 5, 8);
        let mut as_app_sees_it = self_snapshot(&board, SELF_CHECKS);
        for (k, v) in as_app_sees_it.iter_mut() {
            if k == "c" {
                if let Value::Int(n) = v {
                    *n += 6;
                }
            }
        }
        let res = round_check(&board, &as_app_sees_it, None, &[]);
        assert!(res.failed());
        assert!(res.discrepancies.iter().any(|d| d.key == "c"));
    }

    #[test]
    fn missing_spine_blocks_the_round() {
        let full: Vec<(String, Value)> = SPINE.iter().map(|k| (k.as_str().to_string(), Value::Int(1))).collect();
        assert_eq!(missing_spine(&full, SPINE), Vec::<String>::new());
        let just_c = vec![("c".to_string(), Value::Int(1))];
        let missing = missing_spine(&just_c, SPINE);
        let expect: Vec<String> = SPINE.iter().filter(|k| k.as_str() != "c").map(|k| k.as_str().to_string()).collect();
        assert_eq!(missing, expect);
    }

    /// Not supplying a field must not read as agreement.
    #[test]
    fn absent_keys_are_not_silently_passed() {
        let board = midgame(3, 0, 5, 8);
        let c = self_snapshot(&board, &[SelfCheckKey::C]);
        let res = round_check(&board, &c, None, &[]);
        assert!(!res.failed());
        assert!(!missing_spine(&res.reported, SPINE).is_empty());
    }

    // ----------------------------------------------------------- parsing

    #[test]
    fn positional_spine_parses_in_spine_order() {
        let (vals, errs) = parse_self_line("41/12/9/3/5");
        assert!(errs.is_empty());
        assert_eq!(vals, vec![
            ("c".to_string(), Value::Int(41)),
            ("s".to_string(), Value::Int(12)),
            ("str".to_string(), Value::Int(9)),
            ("f".to_string(), Value::Int(3)),
            ("r".to_string(), Value::Int(5)),
        ]);
    }

    #[test]
    fn keyed_form_accepts_any_subset_in_any_order() {
        let (vals, errs) = parse_self_line("c=41 str=9 age=ii row=13");
        assert!(errs.is_empty());
        assert_eq!(get_int(&vals, "c"), Some(41));
        assert_eq!(get_int(&vals, "str"), Some(9));
        assert!(vals.iter().any(|(k, v)| k == "age" && *v == Value::Text("II".to_string())));
        assert_eq!(get_int(&vals, "row"), Some(13));
    }

    #[test]
    fn a_partial_spine_can_leave_gaps() {
        let (vals, errs) = parse_self_line("41//9");
        assert!(errs.is_empty());
        assert_eq!(vals, vec![("c".to_string(), Value::Int(41)), ("str".to_string(), Value::Int(9))]);
    }

    #[test]
    fn an_unknown_key_is_an_error_not_a_shrug() {
        let (vals, errs) = parse_self_line("zz=3");
        assert!(!errs.is_empty());
        assert!(vals.is_empty());
    }

    #[test]
    fn non_numeric_values_are_an_error() {
        let (_, errs) = parse_self_line("c=lots");
        assert!(!errs.is_empty());
    }

    #[test]
    fn too_many_spine_values_is_an_error() {
        let (_, errs) = parse_self_line("1/2/3/4/5/6/7");
        assert!(!errs.is_empty());
    }

    #[test]
    fn a_rival_line_parses_the_player_index_and_values() {
        let (idx, vals, errs) = parse_rival_line("p1 22/4/3/6");
        assert_eq!(idx, Some(1));
        assert!(errs.is_empty());
        assert_eq!(vals, vec![
            ("c".to_string(), Value::Int(22)),
            ("cr".to_string(), Value::Int(4)),
            ("sr".to_string(), Value::Int(3)),
            ("str".to_string(), Value::Int(6)),
        ]);
    }

    #[test]
    fn a_rival_line_accepts_the_keyed_form() {
        let (idx, vals, errs) = parse_rival_line("p2 c=30 str=0");
        assert_eq!(idx, Some(2));
        assert!(errs.is_empty());
        assert_eq!(vals, vec![("c".to_string(), Value::Int(30)), ("str".to_string(), Value::Int(0))]);
    }

    #[test]
    fn a_slash_total_form_takes_the_value_before_the_slash() {
        let (vals, _) = parse_self_line("ca=3/4");
        assert_eq!(get_int(&vals, "ca"), Some(3));
    }

    #[test]
    fn an_unasked_rival_key_is_not_silently_dropped() {
        let (_, vals, errs) = parse_rival_line("p1 c=10 zz=3");
        assert!(!errs.is_empty());
        assert_eq!(vals, vec![("c".to_string(), Value::Int(10))]);
    }

    // --------------------------------------------------- rival consistency

    #[test]
    fn plausible_culture_growth_passes() {
        let mut h = RivalHistory::new();
        assert!(h.check(1, 5, Some(40), Some(6)).is_empty());
        assert!(h.check(1, 6, Some(46), Some(6)).is_empty());
    }

    #[test]
    fn transposed_digits_are_flagged_as_a_warning() {
        let mut h = RivalHistory::new();
        h.check(1, 5, Some(40), Some(6));
        let ds = h.check(1, 6, Some(4), Some(6)); // typed "4" for "46"
        assert_eq!(ds.iter().map(|d| d.severity).collect::<Vec<_>>(), vec![Severity::Warn]);
    }

    #[test]
    fn a_warning_is_never_a_hard_failure() {
        let mut h = RivalHistory::new();
        h.check(1, 5, Some(40), Some(6));
        let board = midgame(3, 0, 5, 8);
        let reported = self_snapshot(&board, SELF_CHECKS);
        let rivals = vec![(1u8, vec![("c".to_string(), Value::Int(4)), ("cr".to_string(), Value::Int(6))])];
        let res = round_check(&board, &reported, Some(&mut h), &rivals);
        assert!(res.warned());
        assert!(!res.failed());
    }

    // ---------------------------------------------- forced rivals are exact

    /// Every key here is also a real `WeightKey` the evaluator prices, and
    /// every `WeightKey` starting with `rival_` that is NOT a per-position
    /// derived feature is named in `NON_FEATURE_RIVAL_WEIGHTS` below -- so
    /// this is the closest Rust equivalent of the Python original's dynamic
    /// `{k for k in feats if k.startswith("rival_")}` introspection over a
    /// dict: Python's features are a dict and can be enumerated at runtime,
    /// Rust's are a fixed struct indexed by the SAME enum as the weights, so
    /// the enumeration has to name its exclusions instead. If the evaluator
    /// grows a new derived `rival_*` feature, this test starts failing until
    /// a human either grows this list (extends the ask) or the exclusion
    /// list (declares it a hyperparameter, a conscious decision) -- the
    /// property a Python-era test once caught a real drift on (that test and
    /// its note are gone with `engine/`; the property is why this Rust test
    /// exists).
    const RIVAL_FEATURE_KEYS: &[WeightKey] = &[
        WeightKey::RivalCulture,
        WeightKey::RivalMeanCulture,
        WeightKey::RivalCultureRate,
        WeightKey::RivalScienceRate,
        WeightKey::RivalStrength,
        WeightKey::RivalFreeCa,
        WeightKey::RivalHandCivil,
        WeightKey::RivalWonders,
        WeightKey::RivalWonderDeficit,
        WeightKey::RivalScienceDeficit,
        WeightKey::RivalCultureDeficit,
        WeightKey::RivalScienceStock,
        WeightKey::RivalFoodStock,
        WeightKey::RivalResourceStock,
        WeightKey::RivalFreeWorkers,
        WeightKey::RivalYellowBank,
        WeightKey::RivalColonies,
        WeightKey::RivalMilActions,
        WeightKey::RivalBuildingWonder,
    ];

    /// Weight keys named `rival_*` that are HYPERPARAMETERS of the
    /// evaluator (read from the weight vector itself, e.g. `w.get
    /// ("rival_desire", ...)`), not per-position features `features()`
    /// computes -- so they are correctly absent from [`RIVAL_FEATURE_KEYS`].
    const NON_FEATURE_RIVAL_WEIGHTS: &[WeightKey] = &[WeightKey::RivalDesire, WeightKey::RivalTakeShare, WeightKey::RivalHandPotential];

    #[test]
    fn every_rival_weight_key_is_accounted_for_as_a_feature_or_a_hyperparameter() {
        let rival_keys: Vec<WeightKey> = WeightKey::ALL.iter().copied().filter(|k| k.name().starts_with("rival_")).collect();
        for k in &rival_keys {
            assert!(
                RIVAL_FEATURE_KEYS.contains(k) || NON_FEATURE_RIVAL_WEIGHTS.contains(k),
                "{}: a new rival_* weight key that is neither a known feature nor a declared \
                 hyperparameter -- decide which it is (see this test's doc comment)",
                k.name()
            );
        }
        for k in RIVAL_FEATURE_KEYS {
            assert!(rival_keys.contains(k), "{}: no longer a WeightKey at all", k.name());
        }
    }

    /// Exactly what the operator types, for every rival, this round: the
    /// forced numbers plus the name channels (wonders/colonies/wonder in
    /// progress). Mirrors the Python test module's `_read_the_panel`.
    fn read_the_panel(st: &GameState, me: u8) -> Vec<PanelRowLocal> {
        let mut out = Vec::new();
        for q in st.players[..st.num_players as usize].iter() {
            if q.idx == me {
                continue;
            }
            let s = effects::compute(st, q);
            let vals = vec![
                ("c".to_string(), Value::Int(q.culture as i64)),
                ("cr".to_string(), Value::Int(s.culture as i64)),
                ("sr".to_string(), Value::Int(s.science as i64)),
                ("str".to_string(), Value::Int(s.strength as i64)),
                ("ca".to_string(), Value::Int(q.civil_actions as i64)),
                ("hc".to_string(), Value::Int(q.hand_size_civil() as i64)),
                ("s".to_string(), Value::Int(q.science as i64)),
                ("f".to_string(), Value::Int(q.food as i64)),
                ("r".to_string(), Value::Int(q.resources as i64)),
                ("fw".to_string(), Value::Int(q.workers_free as i64)),
                ("y".to_string(), Value::Int(q.yellow_bank as i64)),
                ("ma".to_string(), Value::Int(q.military_actions as i64)),
            ];
            let names = vec![
                ("built+", q.completed_wonders.as_slice().iter().map(|c| c.name().to_string()).collect()),
                ("colony+", q.colonies.as_slice().iter().map(|c| c.name().to_string()).collect()),
                ("wonder", if q.wonder.is_none() { Vec::new() } else { vec![format!("{} {}", q.wonder.name(), q.wonder_steps)] }),
            ];
            out.push((q.idx, vals, names));
        }
        out
    }

    /// Every rival board as a completely untranscribed opponent looks: no
    /// workers, no score, no stocks, no wonders, no colonies, no
    /// government, no cards, no actions. Every field the reconstruction
    /// claims to restore must be destroyed HERE. Mirrors `_wreck`.
    fn wreck(st: &mut GameState, me: u8) {
        let despotism = crate::cards::CardId::by_name("Despotism").unwrap();
        let n = st.num_players as usize;
        for q in st.players[..n].iter_mut() {
            if q.idx == me {
                continue;
            }
            let ids: Vec<_> = q.techs.iter().map(|(id, _)| id).collect();
            for id in ids {
                if let Some(slot) = q.techs.get_mut(id) {
                    slot.workers = 0;
                }
            }
            q.culture = 0;
            q.completed_wonders = crate::state::CardList::new();
            q.colonies = crate::state::CardList::new();
            q.wonder = crate::cards::CardId::NONE;
            q.wonder_steps = 0;
            q.government = despotism;
            q.hand_civil = crate::state::CardList::new();
            q.hidden_civil = 0;
            q.civil_actions = 0;
            q.military_actions = 0;
            q.science = 0;
            q.food = 0;
            q.resources = 0;
            q.workers_free = 0;
            q.yellow_bank = 0;
        }
    }

    /// The claim the whole cost estimate rests on: we never mirror an
    /// opponent's board, the human reads a few numbers off the panel and
    /// names the wonders/colonies/wonder-in-progress they see, and
    /// `state_io::patch` back-solves. That is only sound if what we ask for
    /// pins down every rival-derived feature.
    #[test]
    fn what_the_operator_types_reconstructs_every_rival_feature() {
        let mut board = midgame(3, 0, 5, 8);
        let st = &board.state;
        let me = board.me;
        let rival = (me + 1) % st.num_players;
        S::patch(&mut board, &format!("p{rival} built+ Colossus")).unwrap();

        let before = features::features(&board.state, me, None, None, false);
        for k in [WeightKey::RivalFreeCa, WeightKey::RivalHandCivil, WeightKey::RivalWonders] {
            assert!(before.get(k) > 0.0, "{}: is 0 here, so restoring it proves nothing", k.name());
        }

        let panel = read_the_panel(&board.state, me);
        wreck(&mut board.state, me);

        for (idx, vals, names) in &panel {
            for (verb, cards) in names {
                if !cards.is_empty() {
                    S::patch(&mut board, &format!("p{idx} {verb} {}", cards.join(", "))).unwrap();
                }
            }
            for key in RIVAL_FORCE_KEYS {
                let v = get_int(vals, key.as_str()).unwrap();
                S::patch(&mut board, &format!("p{idx} {}={v}", key.as_str())).unwrap();
            }
        }

        let after = features::features(&board.state, me, None, None, false);
        for &k in RIVAL_FEATURE_KEYS {
            assert!(
                (before.get(k) - after.get(k)).abs() < 1e-6,
                "{}: could not be restored from what the operator types",
                k.name()
            );
        }
    }

    #[test]
    fn every_asked_key_is_forced_or_checked_and_forcing_it_never_errors() {
        assert_eq!(RIVAL_ASK_KEYS.len(), RIVAL_FORCE_KEYS.len() + RIVAL_CHECKS.len());
        let board = midgame(3, 0, 5, 8);
        for key in RIVAL_FORCE_KEYS {
            let mut b = board.clone();
            S::patch(&mut b, &format!("p1 {}=3", key.as_str())).unwrap();
        }
    }

    /// A forced key that changes no feature is unpaid data entry.
    #[test]
    fn each_forced_key_actually_moves_its_feature() {
        let moves: &[(RivalAskKey, WeightKey)] = &[
            (RivalAskKey::C, WeightKey::RivalCulture),
            (RivalAskKey::Cr, WeightKey::RivalCultureRate),
            (RivalAskKey::Sr, WeightKey::RivalScienceRate),
            (RivalAskKey::Str, WeightKey::RivalStrength),
            (RivalAskKey::Ca, WeightKey::RivalFreeCa),
            (RivalAskKey::Hc, WeightKey::RivalHandCivil),
            (RivalAskKey::S, WeightKey::RivalScienceStock),
            (RivalAskKey::F, WeightKey::RivalFoodStock),
            (RivalAskKey::R, WeightKey::RivalResourceStock),
            (RivalAskKey::Fw, WeightKey::RivalFreeWorkers),
            (RivalAskKey::Y, WeightKey::RivalYellowBank),
            (RivalAskKey::Ma, WeightKey::RivalMilActions),
        ];
        assert_eq!(moves.len(), RIVAL_FORCE_KEYS.len());
        for &(key, feat) in moves {
            let board = midgame(3, 0, 5, 8);
            let base = features::features(&board.state, board.me, None, None, false).get(feat);
            let mut b = board.clone();
            let n = b.state.num_players;
            for i in 0..n {
                if i != b.me {
                    S::patch(&mut b, &format!("p{i} {}={}", key.as_str(), base as i64 + 9)).unwrap();
                }
            }
            let after = features::features(&b.state, b.me, None, None, false).get(feat);
            assert!((after - (base + 9.0)).abs() < 1e-6, "{}", key.as_str());
        }
    }

    /// Wonders come in by NAME, so the mirror can be wrong about them, and
    /// the count on the panel is a real check -- the only hard one on the
    /// rival side.
    #[test]
    fn a_missed_wonder_is_caught_by_the_count() {
        let board = midgame(3, 0, 5, 8);
        let idx = board.state.players[..board.state.num_players as usize].iter().find(|p| p.idx != board.me).unwrap().idx;
        let mirror_says = board.state.players[idx as usize].completed_wonders.len() as i64;
        let ok = vec![(idx, vec![("w".to_string(), Value::Int(mirror_says))])];
        assert!(check_rivals(&board.state, &ok).is_empty());
        let bad = vec![(idx, vec![("w".to_string(), Value::Int(mirror_says + 1))])];
        let ds = check_rivals(&board.state, &bad);
        assert_eq!(ds.iter().map(|d| (d.key.as_str(), d.severity)).collect::<Vec<_>>(), vec![("w", Severity::Fail)]);

        let mut b2 = board;
        S::patch(&mut b2, &format!("p{idx} built+ Colossus")).unwrap();
        let bad2 = vec![(idx, vec![("w".to_string(), Value::Int(mirror_says + 1))])];
        assert!(check_rivals(&b2.state, &bad2).is_empty());
    }

    /// The ask is static, the derivation is dynamic. Every asked field must
    /// have a probe watching it, or it can stop mattering (or start)
    /// unseen.
    #[test]
    fn every_asked_key_has_a_probe_watching_it() {
        for key in RIVAL_ASK_KEYS {
            let pid = key.probe_id();
            assert!(F::PROBES.iter().any(|p| p.id == pid), "{}: no probe for {pid:?}", key.as_str());
        }
    }
}
