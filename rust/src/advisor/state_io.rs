//! Terse, human-typed text format for a Through the Ages board.
//!
//! Design goal: the human is sitting at a physical table with a phone or
//! laptop next to the board. Typing must be FAST, so
//!
//! * card names are fuzzy-matched -- `pyr` / `loa` / `hang gard` all work
//!   (prefix, initials or subsequence, resolved against the real card table);
//! * nothing has to be repeated between turns -- the advisor keeps a mirror
//!   of the board and the human only reports what CHANGED (`deal` for the
//!   new cards on the right of the row, `p1 c=34` for a rival's culture, ...);
//! * every value may be `?` meaning "I don't know", which leaves the mirror
//!   untouched and records the field as unknown instead of failing.
//!
//! Two representations:
//!
//! [`dumps`] / [`loads`]
//!     the full snapshot, one section per line, round-trips exactly.
//!
//! [`patch`]
//!     a single-line update, the thing the human actually types between
//!     turns.
//!
//! A [`Board`] is the engine [`GameState`] plus the advisor's own
//! book-keeping: which seat the human occupies and which fields the human
//! declared unknown. Ported from `advisor/state_io.py`; see that module's own
//! doc comment for the "hidden count lives on `PlayerState`, not beside it"
//! design note, which this port inherits unchanged (`hidden_civil`/
//! `hidden_military` are already fields on [`PlayerState`] here, not a
//! separate map).
//!
//! ## What changed crossing into Rust
//!
//! * Python caches `effects.compute` on the state and invalidates it by hand
//!   after every mutation (`effects.invalidate`). This port's `effects::
//!   compute` recomputes fresh on every call (see that module's own doc
//!   comment), so there is no `invalidate` step here at all -- every mutating
//!   function below just edits the field and returns.
//! * Errors are `Result<_, String>`, this codebase's convention (see
//!   `bin/arena.rs`), not a `PatchError`/`AmbiguousCard`/`UnknownCard`
//!   exception hierarchy -- that hierarchy existed so Python's REPL could
//!   catch one common base class; a `Result` needs no such thing.
//! * A parsed one-liner becomes a [`Command`] (with a [`PlayerVerb`] for the
//!   `p<N> ...` forms), not a string re-matched at every call site. The
//!   snapshot line grammar shared between [`loads`] and a handful of patch
//!   verbs (`gov`, `tech`, `hand`, `hidden`, `leader`, `tactic`) is its own
//!   [`PlayerLine`] enum for the same reason.
//! * `hc=N`/`hm=N` (a rival's TOTAL hand size, the number printed at the
//!   table) and `hidden hc=N hm=M` (the RAW hidden-card count, what a
//!   snapshot stores) are different operations in Python -- the first
//!   subtracts the cards already named, the second sets the count directly.
//!   That distinction is preserved exactly ([`PlayerField::HiddenCivil`]/
//!   [`PlayerField::HiddenMilitary`] vs [`PlayerLine::Hidden`]): collapsing
//!   them would silently double-count a rival's named hand the next time a
//!   snapshot round-trips.

use std::collections::BTreeSet;

use crate::cards::{Age, CardId, CardType, CARDS};
use crate::economy;
use crate::effects;
use crate::game;
use crate::state::{
    CardList, GameState, PactList, PendingStack, Phase, PlayerState, Queue, Tableau, TechSlot,
    MAX_DECK, MAX_PLAYERS, NOT_SEEDED, ROW_SIZE,
};

pub const VERSION: u32 = 1;
pub const EMPTY: &str = ".";
pub const UNKNOWN: &str = "?";

// ================================================================= board ==

/// One field the human can set on a player line, closed over every alias
/// Python's `SCALARS`/`FORCED` tables accept plus `gov`/`hc`/`hm`. An enum
/// rather than the key string itself so an unrecognised key is rejected once,
/// at parse time, instead of by every function that later matches on it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum PlayerField {
    Gov,
    HiddenCivil,
    HiddenMilitary,
    CivilActions,
    MilitaryActions,
    Food,
    Resources,
    Science,
    Culture,
    BlueTotal,
    YellowBank,
    WorkersFree,
    StrengthExtra,
    HappyExtra,
    CultureRateExtra,
    ScienceRateExtra,
    /// A DERIVED total: setting it backs out the "extra" so the mirror
    /// agrees with `effects::compute`. Mirrors Python's `FORCED["str"]`.
    Strength,
    Happy,
    CultureRate,
    ScienceRate,
}

/// The four [`PlayerField`]s that are set by DERIVED total rather than
/// directly -- Python's `FORCED` table. Its own type (rather than a `_ =>
/// unreachable!()` arm on [`PlayerField`]) so [`force_field`] cannot be
/// called with a field that is not one of these: the four-way match inside
/// it is exhaustive over real possibilities, not defended by a panic.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ForcedField {
    Strength,
    Happy,
    CultureRate,
    ScienceRate,
}

/// Which hand/count a hidden-card operation targets. Replaces Python's
/// `which: str = "civil"` parameter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hand {
    Civil,
    Military,
}

/// What KIND of operation a [`PlayerField`] performs, so [`set_player_field`]
/// can match over four arms instead of nineteen. Mirrors the shape of
/// Python's `_set_player_key`, which branches the same way (gov / hc,hm /
/// FORCED / SCALARS) but on string membership tests instead of a type.
enum PlayerFieldKind {
    Gov,
    Hidden(Hand),
    Forced(ForcedField),
    Scalar,
}

impl PlayerField {
    /// Parse a key token as typed by the human (or dumped by us). Mirrors
    /// Python's `SCALARS`/`FORCED` dicts plus the `gov`/`hc`/`hm` keys
    /// `_set_player_key` also accepts.
    pub fn parse(key: &str) -> Option<PlayerField> {
        use PlayerField::*;
        Some(match key {
            "gov" => Gov,
            "hc" => HiddenCivil,
            "hm" => HiddenMilitary,
            "ca" => CivilActions,
            "ma" => MilitaryActions,
            "f" | "food" => Food,
            "r" | "res" => Resources,
            "s" | "sci" => Science,
            "c" | "cult" => Culture,
            "blue" => BlueTotal,
            "yel" | "y" => YellowBank,
            "fw" => WorkersFree,
            "strx" => StrengthExtra,
            "hapx" => HappyExtra,
            "crx" => CultureRateExtra,
            "srx" => ScienceRateExtra,
            "str" => Strength,
            "hap" => Happy,
            "cr" => CultureRate,
            "sr" => ScienceRate,
            _ => return None,
        })
    }

    /// The canonical spelling used both to print an `unknown p{idx}.{key}`
    /// line and in error messages. Python echoes back whichever alias the
    /// human typed instead; this picks one spelling per field so the
    /// `unknown` set is keyed on the field, not on freeform text (see this
    /// module's top doc comment).
    pub fn short_name(self) -> &'static str {
        use PlayerField::*;
        match self {
            Gov => "gov",
            HiddenCivil => "hc",
            HiddenMilitary => "hm",
            CivilActions => "ca",
            MilitaryActions => "ma",
            Food => "f",
            Resources => "r",
            Science => "s",
            Culture => "c",
            BlueTotal => "blue",
            YellowBank => "yel",
            WorkersFree => "fw",
            StrengthExtra => "strx",
            HappyExtra => "hapx",
            CultureRateExtra => "crx",
            ScienceRateExtra => "srx",
            Strength => "str",
            Happy => "hap",
            CultureRate => "cr",
            ScienceRate => "sr",
        }
    }

    fn kind(self) -> PlayerFieldKind {
        use PlayerField::*;
        match self {
            Gov => PlayerFieldKind::Gov,
            HiddenCivil => PlayerFieldKind::Hidden(Hand::Civil),
            HiddenMilitary => PlayerFieldKind::Hidden(Hand::Military),
            Strength => PlayerFieldKind::Forced(ForcedField::Strength),
            Happy => PlayerFieldKind::Forced(ForcedField::Happy),
            CultureRate => PlayerFieldKind::Forced(ForcedField::CultureRate),
            ScienceRate => PlayerFieldKind::Forced(ForcedField::ScienceRate),
            CivilActions | MilitaryActions | Food | Resources | Science | Culture | BlueTotal
            | YellowBank | WorkersFree | StrengthExtra | HappyExtra | CultureRateExtra
            | ScienceRateExtra => PlayerFieldKind::Scalar,
        }
    }
}

/// A field the human declared `?` for: left untouched in the mirror, but
/// remembered so [`dumps`] can print it back and [`render`] can flag it.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct UnknownField {
    pub player: u8,
    pub field: PlayerField,
}

/// Engine [`GameState`] plus the advisor's own book-keeping about hidden
/// information: which seat the human occupies, and which fields they
/// declared unknown. Mirrors `advisor/state_io.py::Board`.
#[derive(Clone)]
pub struct Board {
    pub state: GameState,
    pub me: u8,
    pub unknown: BTreeSet<UnknownField>,
}

impl Board {
    pub fn player(&self, idx: u8) -> Result<&PlayerState, String> {
        if idx as usize >= self.state.num_players as usize {
            return Err(format!("no player p{idx} in a {}-player game", self.state.num_players));
        }
        Ok(&self.state.players[idx as usize])
    }

    pub fn player_mut(&mut self, idx: u8) -> Result<&mut PlayerState, String> {
        if idx as usize >= self.state.num_players as usize {
            return Err(format!("no player p{idx} in a {}-player game", self.state.num_players));
        }
        Ok(&mut self.state.players[idx as usize])
    }

    pub fn hidden_count(&self, idx: u8, which: Hand) -> Result<u8, String> {
        let p = self.player(idx)?;
        Ok(match which {
            Hand::Civil => p.hidden_civil,
            Hand::Military => p.hidden_military,
        })
    }

    /// Set the hidden-card count directly, clamped at 0 (Python's
    /// `set_hidden`'s `max(0, int(n))`).
    pub fn set_hidden(&mut self, idx: u8, which: Hand, n: i64) -> Result<(), String> {
        let clamped = n.max(0).min(u8::MAX as i64) as u8;
        let p = self.player_mut(idx)?;
        match which {
            Hand::Civil => p.hidden_civil = clamped,
            Hand::Military => p.hidden_military = clamped,
        }
        Ok(())
    }

    /// Cards in hand, named or not -- the quantity the rulebook counts.
    pub fn hand_size(&self, idx: u8, which: Hand) -> Result<usize, String> {
        let p = self.player(idx)?;
        Ok(match which {
            Hand::Civil => p.hand_civil.len() + p.hidden_civil as usize,
            Hand::Military => p.hand_military.len() + p.hidden_military as usize,
        })
    }
}

/// A fresh game mirroring a freshly set-up physical board.
pub fn new_board(num_players: u8, me: u8, seed: u64) -> Board {
    Board { state: game::new_game(num_players, seed), me, unknown: BTreeSet::new() }
}

// ------------------------------------------------------- card resolution --

/// Which cards a slot may hold, e.g. [`Pool::Tech`]. Mirrors `state_io.py`'s
/// `pool(kind)` string tags, closed over the base game's card taxonomy.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pool {
    Any,
    Tech,
    Gov,
    Wonder,
    Leader,
    Tactic,
    Row,
    Military,
    Event,
    Territory,
}

impl Pool {
    fn matches(self, kind: CardType) -> bool {
        match self {
            Pool::Any => true,
            Pool::Tech => kind.takes_workers() || kind == CardType::SpecialTech,
            Pool::Gov => kind == CardType::Government,
            Pool::Wonder => kind == CardType::Wonder,
            Pool::Leader => kind == CardType::Leader,
            Pool::Tactic => kind == CardType::Tactic,
            Pool::Row => kind.is_civil_row(),
            Pool::Military => !kind.is_civil_row(),
            Pool::Event => matches!(kind, CardType::Event | CardType::Territory),
            Pool::Territory => kind == CardType::Territory,
        }
    }

    /// The word used in "no X matches" error messages, matching the `kind`
    /// string every `resolve_card` call site in `state_io.py` passes.
    fn label(self) -> &'static str {
        match self {
            Pool::Any => "card",
            Pool::Tech => "tech",
            Pool::Gov => "gov",
            Pool::Wonder => "wonder",
            Pool::Leader => "leader",
            Pool::Tactic => "tactic",
            Pool::Row => "row",
            Pool::Military => "military",
            Pool::Event => "event",
            Pool::Territory => "territory",
        }
    }
}

// `pub(crate)`, not private: `advisor::advisor`'s `parse_move` needs these
// same three primitives to fuzzy-match a human's typed move argument against
// ONE already-known candidate string (a legal move's own card name, or a
// bare keyword like Churchill's "culture"/"military") -- a narrower job than
// [`resolve_card`]'s "find the one match in a whole pool", which is why it
// calls these directly instead of going through `resolve_card` again.

pub(crate) fn norm(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).filter(|c| c.is_ascii_alphanumeric()).collect()
}

pub(crate) fn initials(name: &str) -> String {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .filter_map(|w| w.chars().next())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Is `needle` a subsequence of `hay` (both already normalised)? Mirrors
/// Python's `it = iter(hay); all(ch in it for ch in needle)` -- the
/// generator's progressive consumption of `it` IS the subsequence check.
pub(crate) fn is_subseq(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|ch| it.by_ref().any(|h| h == ch))
}

/// Fuzzy-match `text` to one card in `pool`, with `extra` candidates
/// searched alongside it (used by `tech-`/`colony-` so a card already in
/// play resolves even if fuzzy-matching against the wider pool would be
/// ambiguous). Tiers, first non-empty wins: raw exact, normalised exact,
/// normalised prefix, initials exact, initials prefix, subsequence.
pub fn resolve_card(text: &str, pool: Pool, extra: &[CardId]) -> Result<CardId, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("no {} matches {:?}", pool.label(), text));
    }
    let mut candidates: Vec<CardId> = extra.to_vec();
    for (i, c) in CARDS.iter().enumerate() {
        if pool.matches(c.kind) {
            candidates.push(CardId(i as u16));
        }
    }
    if let Some(&id) = candidates.iter().find(|&&id| id.name() == text) {
        return Ok(id);
    }
    let q = norm(text);
    let tiers: [Vec<CardId>; 5] = [
        candidates.iter().copied().filter(|&id| norm(id.name()) == q).collect(),
        candidates.iter().copied().filter(|&id| norm(id.name()).starts_with(&q)).collect(),
        candidates.iter().copied().filter(|&id| initials(id.name()) == q).collect(),
        candidates.iter().copied().filter(|&id| initials(id.name()).starts_with(&q)).collect(),
        candidates.iter().copied().filter(|&id| is_subseq(&q, &norm(id.name()))).collect(),
    ];
    for tier in tiers {
        let mut uniq = tier;
        uniq.sort();
        uniq.dedup();
        match uniq.len() {
            0 => continue,
            1 => return Ok(uniq[0]),
            _ => return Err(ambiguous_message(text, &uniq)),
        }
    }
    Err(format!("no {} matches {:?}", pool.label(), text))
}

fn ambiguous_message(text: &str, options: &[CardId]) -> String {
    let mut names: Vec<&str> = options.iter().map(|c| c.name()).collect();
    names.sort_unstable();
    let shown = names.iter().take(8).copied().collect::<Vec<_>>().join(", ");
    let more =
        if names.len() > 8 { format!(" (+{} more)", names.len() - 8) } else { String::new() };
    format!("{text:?} is ambiguous: {shown}{more}")
}

// --------------------------------------------------------- little parsers

/// Split on the first run of whitespace: `("head", "rest, trimmed")`.
fn split_first(s: &str) -> (&str, &str) {
    match s.trim_start().split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim_start()),
        None => (s.trim(), ""),
    }
}

fn parse_kv(text: &str) -> Vec<(&str, &str)> {
    text.split_whitespace().filter_map(|tok| tok.split_once('=')).collect()
}

fn kv_get<'a>(kv: &[(&'a str, &'a str)], key: &str) -> Option<&'a str> {
    kv.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Parse an integer token into `T`, with Python's `_int`'s message.
fn parse_num<T: std::str::FromStr>(tok: &str, key: &str) -> Result<T, String> {
    tok.trim().parse::<T>().map_err(|_| format!("{key}: {tok:?} is not a number"))
}

fn age_str(a: Age) -> &'static str {
    match a {
        Age::A => "A",
        Age::I => "I",
        Age::II => "II",
        Age::III => "III",
        Age::IV => "IV",
    }
}

fn parse_age(s: &str) -> Result<Age, String> {
    match s {
        "A" => Ok(Age::A),
        "I" => Ok(Age::I),
        "II" => Ok(Age::II),
        "III" => Ok(Age::III),
        "IV" => Ok(Age::IV),
        other => Err(format!("age must be one of A, I, II, III, IV, got {other:?}")),
    }
}

fn phase_str(p: Phase) -> &'static str {
    match p {
        Phase::Politics => "politics",
        Phase::Actions => "actions",
        Phase::Done => "done",
    }
}

fn parse_phase(s: &str) -> Result<Phase, String> {
    match s {
        "politics" => Ok(Phase::Politics),
        "actions" => Ok(Phase::Actions),
        "done" => Ok(Phase::Done),
        other => Err(format!("phase must be politics, actions or done, got {other:?}")),
    }
}

/// Comma-separated card list, as written by [`dumps`] -- the strict format,
/// no whitespace-splitting fallback. Mirrors `_split_strict`.
fn split_strict(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
}

/// Split a human-typed card list. Commas are the real separator; without one,
/// whitespace splits UNLESS the whole string is itself one (multi-word) card
/// name in `pool` -- so `deal bro irr alc` still splits three ways but
/// `deal Hanging Gardens` does not. Mirrors `_split_cards`.
///
/// `pub`, not `pub(crate)`: `bin/advisor.rs`'s console (a separate crate
/// that only depends on this one) needs the identical split for its "new
/// cards dealt" prompt, for the same reason `patch`'s `deal` verb does -- it
/// is the same human typing the same shorthand at a different prompt, not a
/// different grammar.
pub fn split_cards(text: &str, pool: Option<Pool>) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    if text.contains(',') {
        return split_strict(text);
    }
    if let Some(p) = pool {
        if text.contains(' ') && resolve_card(text, p, &[]).is_ok() {
            return vec![text.to_string()];
        }
    }
    text.split_whitespace().map(str::to_string).collect()
}

fn resolve_names(tokens: &[String], pool: Pool) -> Result<Vec<CardId>, String> {
    tokens.iter().map(|t| resolve_card(t, pool, &[])).collect()
}

fn resolve_names_skip_empty(tokens: &[String], pool: Pool) -> Result<Vec<CardId>, String> {
    tokens.iter().filter(|t| t.as_str() != EMPTY).map(|t| resolve_card(t, pool, &[])).collect()
}

/// `.`/`-`/`(empty)` in a slot that means "not typed / cleared", the
/// convention every clearable field (`wonder -`, `leader -`, `colony -`)
/// shares.
fn is_clear_marker(tail: &str) -> bool {
    matches!(tail, "" | "-" | EMPTY)
}

fn fill_cardlist<const N: usize>(names: &[CardId]) -> CardList<N> {
    let mut out = CardList::new();
    for &id in names {
        out.push(id);
    }
    out
}

fn join_names(cards: &[CardId]) -> String {
    cards.iter().map(|c| c.name()).collect::<Vec<_>>().join(", ")
}

fn fmt_hand_sorted(cards: &[CardId]) -> String {
    if cards.is_empty() {
        return EMPTY.to_string();
    }
    let mut names: Vec<&str> = cards.iter().map(|c| c.name()).collect();
    names.sort_unstable();
    names.join(", ")
}

/// `name:workers` (Python's `tok.rpartition(":")`): no colon means the whole
/// token is the name and `default_workers` fills in for the count.
fn split_tech_token(tok: &str, default_workers: &str) -> (String, String) {
    match tok.rfind(':') {
        Some(i) if i > 0 => (tok[..i].to_string(), tok[i + 1..].to_string()),
        _ => (tok.to_string(), default_workers.to_string()),
    }
}

/// `NAME steps` where `steps` is a trailing integer, else `NAME` with steps
/// 0. Mirrors `tail.rsplit(None, 1)` plus an `isdigit()` guard.
fn parse_wonder_tail(tail: &str) -> (String, u8) {
    let trimmed = tail.trim();
    if let Some(i) = trimmed.rfind(char::is_whitespace) {
        let (name, num) = (trimmed[..i].trim(), trimmed[i + 1..].trim());
        if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) {
            if let Ok(steps) = num.parse::<u8>() {
                return (name.to_string(), steps);
            }
        }
    }
    (trimmed.to_string(), 0)
}

fn parse_player_word(w: &str) -> Option<u8> {
    let rest = w.strip_prefix('p')?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// `p<N>` (or bare `<N>`), Python's `_player_idx`.
fn parse_player_idx(tok: &str) -> Result<u8, String> {
    let t = tok.trim().to_ascii_lowercase();
    let t = t.trim_start_matches('p');
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("expected a player like p1, got {tok:?}"));
    }
    t.parse().map_err(|_| format!("expected a player like p1, got {tok:?}"))
}

// ----------------------------------------------------------- the card row

/// Force the row to `names` (`CardId::NONE` for an empty slot), keeping the
/// internal deck's *composition* honest: a card we dealt but that is not
/// really on the table goes back into the deck, and one that really is on
/// the table is taken out of it. Mirrors `sync_row`.
pub fn sync_row(board: &mut Board, names: &[CardId]) {
    let mut row = [CardId::NONE; ROW_SIZE];
    for (slot, &name) in row.iter_mut().zip(names.iter().chain(std::iter::repeat(&CardId::NONE)))
    {
        *slot = name;
    }
    let old: Vec<CardId> =
        board.state.card_row.iter().copied().filter(|c| !c.is_none()).collect();
    let new: Vec<CardId> = row.iter().copied().filter(|c| !c.is_none()).collect();
    for &n in &new {
        if board.state.civil_deck.contains(n) && !old.contains(&n) {
            board.state.civil_deck.remove_first(n);
        }
    }
    for &n in &old {
        if !new.contains(&n) && !board.state.civil_deck.contains(n) {
            board.state.civil_deck.push(n);
        }
    }
    board.state.card_row = row;
}

/// The start-of-turn row shuffle, driven by what the human reports: discard
/// the leftmost `sweep` cards (3/2/1 by live player count when `sweep` is
/// `None`), slide the rest left, and fill the empty slots left-to-right with
/// `new_cards`. Mirrors `advance_row`.
pub fn advance_row(
    board: &mut Board,
    new_cards: &[CardId],
    sweep: Option<usize>,
) -> Result<[CardId; ROW_SIZE], String> {
    let sweep = sweep.unwrap_or_else(|| game::sweep_count(game::live_count(&board.state)));
    let mut row = board.state.card_row;
    for slot in row.iter_mut().take(sweep.min(ROW_SIZE)) {
        *slot = CardId::NONE;
    }
    let mut kept: Vec<CardId> = row.iter().copied().filter(|c| !c.is_none()).collect();
    kept.resize(ROW_SIZE, CardId::NONE);
    let mut row: [CardId; ROW_SIZE] = kept.try_into().expect("resized to ROW_SIZE");
    for &name in new_cards {
        match row.iter_mut().find(|c| c.is_none()) {
            Some(slot) => *slot = name,
            None => return Err("more new cards than empty row slots".to_string()),
        }
    }
    sync_row(board, &row);
    Ok(row)
}

// ------------------------------------------------------------ dump / load

/// The full snapshot. `loads(&dumps(b)) `reproduces `b`'s displayed state
/// exactly: `dumps(loads(x)) == x` for any `x` this module produced.
pub fn dumps(board: &Board) -> String {
    let st = &board.state;
    let mut out = Vec::new();
    out.push(format!("tta {VERSION}"));
    out.push(format!(
        "game {}p seed={} turn={} round={} age={}/{} cur={} start={} phase={} me={}",
        st.num_players,
        st.seed,
        st.turn,
        st.round,
        age_str(st.age_civil),
        age_str(st.age_military),
        st.current,
        st.start_player,
        phase_str(st.phase),
        board.me,
    ));
    out.push(format!("deck civil={} mil={}", st.civil_deck.len(), st.military_deck.len()));
    let row: Vec<&str> =
        st.card_row.iter().map(|&c| if c.is_none() { EMPTY } else { c.name() }).collect();
    out.push(format!("row {}", row.join(", ")));
    out.push(format!(
        "events age={} fut={} past={}",
        age_str(st.current_events_age),
        st.future_events.len(),
        st.past_events.len()
    ));
    if !st.current_events.is_empty() {
        out.push(format!("curev {}", join_names(st.current_events.as_slice())));
    }
    if !st.available_tactics.is_empty() {
        out.push(format!("tactics {}", join_names(st.available_tactics.as_slice())));
    }
    if st.last_round {
        out.push("last_round".to_string());
    }
    for p in st.players[..st.num_players as usize].iter() {
        dump_player(&mut out, board, p);
    }
    for u in &board.unknown {
        out.push(format!("unknown p{}.{}", u.player, u.field.short_name()));
    }
    out.join("\n") + "\n"
}

fn dump_player(out: &mut Vec<String>, board: &Board, p: &PlayerState) {
    let tag = if p.idx == board.me { " me" } else { "" };
    let mut flags = String::new();
    if p.politics_done {
        flags.push_str(" pol");
    }
    if p.resigned {
        flags.push_str(" resigned");
    }
    out.push(format!(
        "p{}{tag} ca={} ma={} f={} r={} s={} c={} blue={} yel={} fw={} strx={} hapx={} crx={} srx={}{flags}",
        p.idx, p.civil_actions, p.military_actions, p.food, p.resources, p.science, p.culture,
        p.blue_total, p.yellow_bank, p.workers_free, p.strength_extra, p.happy_extra,
        p.culture_rate_extra, p.science_rate_extra,
    ));
    out.push(format!("p{} gov {}", p.idx, p.government.name()));
    if !p.techs.is_empty() {
        let mut techs: Vec<(&str, u8)> =
            p.techs.iter().map(|(id, slot)| (id.name(), slot.workers)).collect();
        techs.sort_unstable_by_key(|&(n, _)| n);
        let text =
            techs.iter().map(|(n, w)| format!("{n}:{w}")).collect::<Vec<_>>().join(", ");
        out.push(format!("p{} tech {text}", p.idx));
    }
    if !p.hand_civil.is_empty() || !p.hand_military.is_empty() {
        out.push(format!(
            "p{} hand {} | {}",
            p.idx,
            fmt_hand_sorted(p.hand_civil.as_slice()),
            fmt_hand_sorted(p.hand_military.as_slice())
        ));
    }
    if p.hidden_civil > 0 || p.hidden_military > 0 {
        out.push(format!("p{} hidden hc={} hm={}", p.idx, p.hidden_civil, p.hidden_military));
    }
    if !p.wonder.is_none() {
        out.push(format!("p{} wonder {} {}", p.idx, p.wonder.name(), p.wonder_steps));
    }
    if !p.completed_wonders.is_empty() {
        out.push(format!("p{} built {}", p.idx, join_names(p.completed_wonders.as_slice())));
    }
    if !p.colonies.is_empty() {
        out.push(format!("p{} colony {}", p.idx, join_names(p.colonies.as_slice())));
    }
    if !p.leader.is_none() {
        out.push(format!("p{} leader {}", p.idx, p.leader.name()));
    }
    if !p.tactic.is_none() {
        let star = if p.tactic_exclusive { "*" } else { "" };
        out.push(format!("p{} tactic {}{star}", p.idx, p.tactic.name()));
    }
}

/// Parse a snapshot produced by [`dumps`] (or hand-written in the same
/// format).
pub fn loads(text: &str) -> Result<Board, String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if lines.is_empty() || !lines[0].starts_with("tta") {
        return Err("snapshot must start with a 'tta <version>' line".to_string());
    }
    let game_line = lines
        .iter()
        .find(|l| l.starts_with("game "))
        .ok_or_else(|| "snapshot has no 'game' line".to_string())?;
    let head = parse_kv(&game_line["game ".len()..]);
    let n_text = kv_get(&head, "np")
        .or_else(|| kv_get(&head, "players"))
        .map(str::to_string)
        .or_else(|| players_token(game_line))
        .ok_or_else(|| "game line must say how many players, e.g. '3p'".to_string())?;
    let n: u8 = parse_num(&n_text, "players")?;
    let seed: u64 = parse_num(kv_get(&head, "seed").unwrap_or("0"), "seed")?;
    let mut board = Board { state: blank_state(n, seed), me: 0, unknown: BTreeSet::new() };

    // The deck line is applied LAST: it only carries counts, and loading the
    // row adjusts deck composition, which would otherwise change the count.
    let mut deck_line: Option<&str> = None;
    for &ln in &lines[1..] {
        let (word, _) = split_first(ln);
        if word == "deck" {
            deck_line = Some(ln);
            continue;
        }
        load_line(&mut board, ln)?;
    }
    if let Some(ln) = deck_line {
        load_line(&mut board, ln)?;
    }
    Ok(board)
}

/// Fall back to the `<N>p` token in the `game` line when neither `np=` nor
/// `players=` was given -- the format [`dumps`] itself writes.
fn players_token(game_line: &str) -> Option<String> {
    game_line.split_whitespace().find_map(|tok| {
        let digits = tok.strip_suffix('p')?;
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            Some(digits.to_string())
        } else {
            None
        }
    })
}

fn blank_state(n: u8, seed: u64) -> GameState {
    let despotism = CardId::by_name("Despotism").expect("card table has Despotism");
    GameState {
        num_players: n,
        seed,
        players: std::array::from_fn(|i| blank_player(i as u8, despotism)),
        current: 0,
        turn: 1,
        round: 1,
        start_player: 0,
        age_civil: Age::A,
        age_military: Age::A,
        civil_deck: CardList::new(),
        military_deck: CardList::new(),
        card_row: [CardId::NONE; ROW_SIZE],
        future_events: CardList::new(),
        current_events: CardList::new(),
        past_events: CardList::new(),
        current_events_age: Age::A,
        seeded_by: [NOT_SEEDED; crate::cards::NUM_CARDS],
        scoring_events: CardList::new(),
        available_tactics: CardList::new(),
        civil_discard: std::array::from_fn(|_| CardList::new()),
        civil_removed: std::array::from_fn(|_| CardList::new()),
        discarded_military: std::array::from_fn(|_| CardList::new()),
        last_round: false,
        final_round_end: None,
        game_over: false,
        phase: Phase::Politics,
        forced_winner: None,
        pending: PendingStack::new(),
        queue: Queue::new(),
    }
}

/// A blank player, matching `engine/state.py::PlayerState`'s dataclass
/// defaults field for field -- so a snapshot line the human never types
/// (leader, wonder, colonies, ...) leaves that field exactly where a fresh
/// `PlayerState(idx=i)` would.
fn blank_player(idx: u8, government: CardId) -> PlayerState {
    PlayerState {
        idx,
        techs: Tableau::new(),
        government,
        leader: CardId::NONE,
        used_leader_ability: false,
        wonder: CardId::NONE,
        wonder_steps: 0,
        completed_wonders: CardList::new(),
        destroyed_wonders: 0,
        homer_wonder: CardId::NONE,
        tactic: CardId::NONE,
        tactic_exclusive: false,
        colonies: CardList::new(),
        flipped_wonders: CardList::new(),
        taken_leader_ages: 0,
        war_declared_by_me: CardId::NONE,
        war_target: 0,
        wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
        pacts: PactList::new(),
        hand_civil: CardList::new(),
        hand_military: CardList::new(),
        hidden_civil: 0,
        hidden_military: 0,
        yellow_bank: 18,
        yellow_granted: 0,
        workers_free: 1,
        blue_total: 16,
        food: 0,
        resources: 0,
        science: 0,
        culture: 0,
        culture_rate_extra: 0,
        science_rate_extra: 0,
        strength_extra: 0,
        happy_extra: 0,
        civil_actions: 4,
        military_actions: 2,
        politics_done: false,
        tactic_action_used: false,
        taken_this_turn: CardList::new(),
        ca_spent_taking: 0,
        hammurabi_used: false,
        churchill_used: false,
        bach_upgrade_used: false,
        ocean_liners_used: false,
        caesar_double_politics_used: false,
        skip_next_politics: false,
        caesar_second_politics: false,
        peeked_event: CardId::NONE,
        ca_penalty_next_turn: 0,
        mil_discount: 0,
        mil_sci_discount: 0,
        one_time_discount: crate::state::OneTimeDiscount::default(),
        resigned: false,
    }
}

fn load_line(board: &mut Board, ln: &str) -> Result<(), String> {
    let (word, rest) = split_first(ln);
    if let Some(idx) = parse_player_word(word) {
        let line = parse_player_line(rest)?;
        apply_player_line(board, idx, line)?;
        return Ok(());
    }
    match word {
        "game" => load_game_line(board, rest),
        "deck" => load_deck_line(board, rest),
        "row" => {
            let names = resolve_row_tokens(&split_strict(rest))?;
            sync_row(board, &names);
            Ok(())
        }
        "events" => load_events_line(board, rest),
        "curev" => {
            board.state.current_events =
                fill_cardlist(&resolve_names(&split_strict(rest), Pool::Military)?);
            Ok(())
        }
        "tactics" => {
            board.state.available_tactics =
                fill_cardlist(&resolve_names(&split_strict(rest), Pool::Tactic)?);
            Ok(())
        }
        "last_round" => {
            board.state.last_round = true;
            Ok(())
        }
        "unknown" => {
            board.unknown.insert(parse_unknown(rest)?);
            Ok(())
        }
        _ => Err(format!("don't understand line {ln:?}")),
    }
}

fn resolve_row_tokens(tokens: &[String]) -> Result<Vec<CardId>, String> {
    tokens.iter().map(|t| if t == EMPTY { Ok(CardId::NONE) } else { resolve_card(t, Pool::Row, &[]) }).collect()
}

fn parse_unknown(rest: &str) -> Result<UnknownField, String> {
    let rest = rest.trim();
    let (p_tok, key) =
        rest.split_once('.').ok_or_else(|| format!("bad unknown line {rest:?}"))?;
    let player = parse_player_idx(p_tok)?;
    let field = PlayerField::parse(key).ok_or_else(|| format!("unknown player key {key:?}"))?;
    Ok(UnknownField { player, field })
}

fn load_game_line(board: &mut Board, rest: &str) -> Result<(), String> {
    let kv = parse_kv(rest);
    if let Some(v) = kv_get(&kv, "turn") {
        board.state.turn = parse_num(v, "turn")?;
    }
    if let Some(v) = kv_get(&kv, "round") {
        board.state.round = parse_num(v, "round")?;
    }
    if let Some(v) = kv_get(&kv, "cur") {
        board.state.current = parse_num(v, "cur")?;
    }
    if let Some(v) = kv_get(&kv, "start") {
        board.state.start_player = parse_num(v, "start")?;
    }
    if let Some(v) = kv_get(&kv, "seed") {
        board.state.seed = parse_num(v, "seed")?;
    }
    if let Some(v) = kv_get(&kv, "age") {
        let (civ, mil) = v.split_once('/').unwrap_or((v, v));
        board.state.age_civil = parse_age(civ)?;
        board.state.age_military = parse_age(mil)?;
    }
    if let Some(v) = kv_get(&kv, "phase") {
        board.state.phase = parse_phase(v)?;
    }
    if let Some(v) = kv_get(&kv, "me") {
        board.me = parse_num(v, "me")?;
    }
    Ok(())
}

fn load_deck_line(board: &mut Board, rest: &str) -> Result<(), String> {
    let kv = parse_kv(rest);
    let civil_n: usize = parse_num(kv_get(&kv, "civil").unwrap_or("0"), "civil")?;
    let mil_n: usize = parse_num(kv_get(&kv, "mil").unwrap_or("0"), "mil")?;
    board.state.civil_deck =
        fake_deck(board.state.age_civil, board.state.num_players, civil_n, false)?;
    board.state.military_deck =
        fake_deck(board.state.age_military, board.state.num_players, mil_n, true)?;
    Ok(())
}

/// A deck of the right SIZE (identities are hidden from the human): the real
/// age deck, trimmed or padded. Padding repeats the first card rather than
/// leaving the deck short, matching Python's `_fake_deck` -- only the COUNT
/// is load-bearing (it decides when the age ends), never the identity.
fn fake_deck(age: Age, num_players: u8, size: usize, military: bool) -> Result<CardList<MAX_DECK>, String> {
    if size > MAX_DECK {
        return Err(format!("deck of {size} cards exceeds the engine's bound of {MAX_DECK}"));
    }
    let n = (num_players as usize).clamp(2, 4);
    let names = game::build_deck(age, !military, n);
    let mut out = CardList::new();
    if names.len() >= size {
        for &c in names.as_slice().iter().take(size) {
            out.push(c);
        }
    } else {
        for &c in names.as_slice() {
            out.push(c);
        }
        let filler = names
            .as_slice()
            .first()
            .copied()
            .unwrap_or_else(|| CardId::by_name("Bronze").expect("Bronze exists"));
        for _ in names.len()..size {
            out.push(filler);
        }
    }
    Ok(out)
}

fn load_events_line(board: &mut Board, rest: &str) -> Result<(), String> {
    let kv = parse_kv(rest);
    if let Some(v) = kv_get(&kv, "age") {
        board.state.current_events_age = parse_age(v)?;
    }
    let fut: usize = parse_num(kv_get(&kv, "fut").unwrap_or("0"), "fut")?;
    let past: usize = parse_num(kv_get(&kv, "past").unwrap_or("0"), "past")?;
    board.state.future_events = unknown_events(fut)?;
    board.state.past_events = unknown_events(past)?;
    Ok(())
}

/// `n` events of unknown identity -- only the COUNT is real information
/// (Python stores `["?"] * n`); `CardId::NONE` is the closest sentinel this
/// port has for "a card is here, but not one we know the name of", and
/// nothing downstream of `state_io` reads these lists by content, only by
/// `len()`.
fn unknown_events(n: usize) -> Result<CardList<MAX_DECK>, String> {
    if n > MAX_DECK {
        return Err(format!("{n} events exceeds the engine's bound of {MAX_DECK}"));
    }
    let mut out = CardList::new();
    for _ in 0..n {
        out.push(CardId::NONE);
    }
    Ok(out)
}

// ------------------------------------------------- the shared line grammar
//
// The syntax `loads` uses for a player's lines (`p1 gov ...`, `p1 tech
// ...`), reused verbatim by six of `patch`'s player verbs (`gov`, `tech`,
// `hand`, `hidden`, `leader`, `tactic` -- see `PlayerVerb::Delegate`) because
// that syntax IS the patch syntax for those six, not merely similar to it.

enum PlayerLine {
    Gov(CardId),
    /// Full replace, the "tech" head. Mirrors `_load_player_line`'s `tech`
    /// branch and NOT `tech+`'s incremental add -- see [`PlayerVerb::TechAdd`].
    Tech(Vec<(CardId, u8)>),
    Hand { civil: Vec<CardId>, military: Vec<CardId> },
    /// The RAW hidden count, set directly -- what a snapshot line means, as
    /// opposed to `PlayerField::HiddenCivil`'s "total minus named cards".
    Hidden { hc: u32, hm: u32 },
    Wonder { card: CardId, steps: u8 },
    /// Full replace.
    Built(Vec<CardId>),
    /// Full replace.
    Colony(Vec<CardId>),
    /// `CardId::NONE` clears.
    Leader(CardId),
    /// `CardId::NONE` clears (and `exclusive` is meaningless then).
    Tactic { card: CardId, exclusive: bool },
    Scalars(Vec<PlayerToken>),
}

impl PlayerLine {
    /// The word `patch`'s generic "p{idx} {head} updated" confirmation names
    /// -- Python discards `_load_player_line`'s own (nonexistent) return
    /// value on this path and always prints the head word instead.
    fn head_word(&self) -> &'static str {
        match self {
            PlayerLine::Gov(_) => "gov",
            PlayerLine::Tech(_) => "tech",
            PlayerLine::Hand { .. } => "hand",
            PlayerLine::Hidden { .. } => "hidden",
            PlayerLine::Wonder { .. } => "wonder",
            PlayerLine::Built(_) => "built",
            PlayerLine::Colony(_) => "colony",
            PlayerLine::Leader(_) => "leader",
            PlayerLine::Tactic { .. } => "tactic",
            PlayerLine::Scalars(_) => "",
        }
    }
}

enum PlayerFlag {
    Politics,
    Resigned,
}

enum PlayerToken {
    Me,
    Flag(PlayerFlag),
    Field { field: PlayerField, raw: String },
}

/// Parse one player's line body (everything after `p<N> `). Mirrors
/// `_load_player_line`.
fn parse_player_line(rest: &str) -> Result<PlayerLine, String> {
    let (head, tail) = split_first(rest);
    match head {
        "gov" => Ok(PlayerLine::Gov(resolve_card(tail, Pool::Gov, &[])?)),
        "tech" => Ok(PlayerLine::Tech(parse_tech_list(tail, "0")?)),
        "hand" => {
            let (civil_txt, mil_txt) = tail.split_once('|').unwrap_or((tail, ""));
            Ok(PlayerLine::Hand {
                civil: resolve_names_skip_empty(&split_strict(civil_txt), Pool::Row)?,
                military: resolve_names_skip_empty(&split_strict(mil_txt), Pool::Military)?,
            })
        }
        "hidden" => {
            let kv = parse_kv(tail);
            Ok(PlayerLine::Hidden {
                hc: parse_num(kv_get(&kv, "hc").unwrap_or("0"), "hc")?,
                hm: parse_num(kv_get(&kv, "hm").unwrap_or("0"), "hm")?,
            })
        }
        "wonder" => {
            let (name, steps) = parse_wonder_tail(tail);
            Ok(PlayerLine::Wonder { card: resolve_card(&name, Pool::Wonder, &[])?, steps })
        }
        "built" => Ok(PlayerLine::Built(resolve_names(&split_strict(tail), Pool::Wonder)?)),
        "colony" => Ok(PlayerLine::Colony(resolve_names(&split_strict(tail), Pool::Territory)?)),
        "leader" => {
            let t = tail.trim();
            Ok(PlayerLine::Leader(if is_clear_marker(t) {
                CardId::NONE
            } else {
                resolve_card(t, Pool::Leader, &[])?
            }))
        }
        "tactic" => {
            let t = tail.trim();
            let exclusive = t.ends_with('*');
            let t = t.trim_end_matches('*');
            if is_clear_marker(t) {
                Ok(PlayerLine::Tactic { card: CardId::NONE, exclusive: false })
            } else {
                Ok(PlayerLine::Tactic { card: resolve_card(t, Pool::Tactic, &[])?, exclusive })
            }
        }
        _ => Ok(PlayerLine::Scalars(parse_player_tokens(rest)?)),
    }
}

fn parse_tech_list(tail: &str, default_workers: &str) -> Result<Vec<(CardId, u8)>, String> {
    parse_tech_tokens(&split_strict(tail), default_workers)
}

fn parse_tech_tokens(tokens: &[String], default_workers: &str) -> Result<Vec<(CardId, u8)>, String> {
    let mut out: Vec<(CardId, u8)> = Vec::new();
    for tok in tokens {
        let (name, workers) = split_tech_token(tok, default_workers);
        let id = resolve_card(&name, Pool::Tech, &[])?;
        let w: u8 = parse_num(&workers, "workers")?;
        match out.iter_mut().find(|(existing, _)| *existing == id) {
            Some(entry) => entry.1 = w,
            None => out.push((id, w)),
        }
    }
    Ok(out)
}

fn parse_player_tokens(rest: &str) -> Result<Vec<PlayerToken>, String> {
    let mut out = Vec::new();
    for tok in rest.split_whitespace() {
        if tok == "me" {
            out.push(PlayerToken::Me);
            continue;
        }
        if tok == "pol" {
            out.push(PlayerToken::Flag(PlayerFlag::Politics));
            continue;
        }
        if tok == "resigned" {
            out.push(PlayerToken::Flag(PlayerFlag::Resigned));
            continue;
        }
        let (k, v) = tok
            .split_once('=')
            .ok_or_else(|| format!("expected key=value, got {tok:?}"))?;
        let field = PlayerField::parse(k)
            .ok_or_else(|| format!("unknown player key {k:?} (try: {})", settable_keys_hint()))?;
        out.push(PlayerToken::Field { field, raw: v.to_string() });
    }
    Ok(out)
}

fn settable_keys_hint() -> String {
    let mut names: Vec<&str> = SETTABLE_KEYS.iter().map(|f| f.short_name()).collect();
    names.sort_unstable();
    names.join(", ")
}

const SETTABLE_KEYS: &[PlayerField] = &[
    PlayerField::CivilActions,
    PlayerField::MilitaryActions,
    PlayerField::Food,
    PlayerField::Resources,
    PlayerField::Science,
    PlayerField::Culture,
    PlayerField::BlueTotal,
    PlayerField::YellowBank,
    PlayerField::WorkersFree,
    PlayerField::StrengthExtra,
    PlayerField::HappyExtra,
    PlayerField::CultureRateExtra,
    PlayerField::ScienceRateExtra,
    PlayerField::Strength,
    PlayerField::Happy,
    PlayerField::CultureRate,
    PlayerField::ScienceRate,
];

fn apply_player_line(board: &mut Board, idx: u8, line: PlayerLine) -> Result<String, String> {
    board.player(idx)?;
    match line {
        PlayerLine::Gov(id) => {
            board.player_mut(idx)?.government = id;
            Ok(format!("p{idx} government = {}", id.name()))
        }
        PlayerLine::Tech(list) => {
            let p = board.player_mut(idx)?;
            p.techs = Tableau::new();
            for (id, w) in &list {
                p.techs.insert(*id, TechSlot { workers: *w, stored: 0 });
            }
            Ok(format!("p{idx} tech updated"))
        }
        PlayerLine::Hand { civil, military } => {
            let p = board.player_mut(idx)?;
            p.hand_civil = fill_cardlist(&civil);
            p.hand_military = fill_cardlist(&military);
            Ok(format!("p{idx} hand updated"))
        }
        PlayerLine::Hidden { hc, hm } => {
            board.set_hidden(idx, Hand::Civil, hc as i64)?;
            board.set_hidden(idx, Hand::Military, hm as i64)?;
            Ok(format!("p{idx} hidden hc={hc} hm={hm}"))
        }
        PlayerLine::Wonder { card, steps } => {
            let p = board.player_mut(idx)?;
            p.wonder = card;
            p.wonder_steps = steps;
            Ok(format!("p{idx} wonder {} {steps} step(s)", card.name()))
        }
        PlayerLine::Built(list) => {
            board.player_mut(idx)?.completed_wonders = fill_cardlist(&list);
            Ok(format!("p{idx} built updated"))
        }
        PlayerLine::Colony(list) => {
            board.player_mut(idx)?.colonies = fill_cardlist(&list);
            Ok(format!("p{idx} colony updated"))
        }
        PlayerLine::Leader(id) => {
            board.player_mut(idx)?.leader = id;
            Ok(if id.is_none() {
                format!("p{idx} has no leader")
            } else {
                format!("p{idx} leader = {}", id.name())
            })
        }
        PlayerLine::Tactic { card, exclusive } => {
            let p = board.player_mut(idx)?;
            p.tactic = card;
            p.tactic_exclusive = exclusive;
            Ok(if card.is_none() {
                format!("p{idx} has no tactic")
            } else {
                format!("p{idx} tactic = {}", card.name())
            })
        }
        PlayerLine::Scalars(tokens) => apply_player_tokens(board, idx, tokens),
    }
}

fn apply_player_tokens(board: &mut Board, idx: u8, tokens: Vec<PlayerToken>) -> Result<String, String> {
    let mut msgs = Vec::new();
    for tok in tokens {
        match tok {
            PlayerToken::Me => board.me = idx,
            PlayerToken::Flag(PlayerFlag::Politics) => {
                board.player_mut(idx)?.politics_done = true;
            }
            PlayerToken::Flag(PlayerFlag::Resigned) => {
                board.player_mut(idx)?.resigned = true;
            }
            PlayerToken::Field { field, raw } => {
                msgs.push(set_player_field(board, idx, field, &raw)?);
            }
        }
    }
    Ok(msgs.join("; "))
}

/// One `key=value` token on a player line. Mirrors `_set_player_key`.
fn set_player_field(board: &mut Board, idx: u8, field: PlayerField, raw: &str) -> Result<String, String> {
    if raw == UNKNOWN {
        board.unknown.insert(UnknownField { player: idx, field });
        return Ok(format!("p{idx}.{} unknown", field.short_name()));
    }
    board.unknown.remove(&UnknownField { player: idx, field });
    match field.kind() {
        PlayerFieldKind::Gov => {
            let id = resolve_card(raw, Pool::Gov, &[])?;
            board.player_mut(idx)?.government = id;
            Ok(format!("p{idx} government = {}", id.name()))
        }
        PlayerFieldKind::Hidden(which) => {
            let total: i64 = parse_num(raw, field.short_name())?;
            let known = board.hand_size(idx, which)? as i64;
            board.set_hidden(idx, which, total - known)?;
            let label = match which {
                Hand::Civil => "civil",
                Hand::Military => "military",
            };
            Ok(format!("p{idx} {label} hand = {}", board.hand_size(idx, which)?))
        }
        PlayerFieldKind::Forced(forced) => {
            let value: i32 = parse_num(raw, field.short_name())?;
            force_field(board, idx, forced, value)?;
            Ok(format!("p{idx} {} = {raw}", field.short_name()))
        }
        PlayerFieldKind::Scalar => {
            let v = raw.split('/').next().unwrap_or(raw); // accept "ca=3/4"
            set_scalar(board.player_mut(idx)?, field, v)?;
            Ok(format!("p{idx} {} = {v}", field.short_name()))
        }
    }
}

fn set_scalar(p: &mut PlayerState, field: PlayerField, v: &str) -> Result<(), String> {
    let key = field.short_name();
    match field {
        PlayerField::CivilActions => p.civil_actions = parse_num(v, key)?,
        PlayerField::MilitaryActions => p.military_actions = parse_num(v, key)?,
        PlayerField::Food => p.food = parse_num(v, key)?,
        PlayerField::Resources => p.resources = parse_num(v, key)?,
        PlayerField::Science => p.science = parse_num(v, key)?,
        PlayerField::Culture => p.culture = parse_num(v, key)?,
        PlayerField::BlueTotal => p.blue_total = parse_num(v, key)?,
        PlayerField::YellowBank => p.yellow_bank = parse_num(v, key)?,
        PlayerField::WorkersFree => p.workers_free = parse_num(v, key)?,
        PlayerField::StrengthExtra => p.strength_extra = parse_num(v, key)?,
        PlayerField::HappyExtra => p.happy_extra = parse_num(v, key)?,
        PlayerField::CultureRateExtra => p.culture_rate_extra = parse_num(v, key)?,
        PlayerField::ScienceRateExtra => p.science_rate_extra = parse_num(v, key)?,
        PlayerField::Gov
        | PlayerField::HiddenCivil
        | PlayerField::HiddenMilitary
        | PlayerField::Strength
        | PlayerField::Happy
        | PlayerField::CultureRate
        | PlayerField::ScienceRate => {
            unreachable!("set_scalar called with a non-scalar field: {field:?}")
        }
    }
    Ok(())
}

/// Set a DERIVED total by backing out the "extra" against
/// `effects::compute`'s base value. Mirrors `_force`.
fn force_field(board: &mut Board, idx: u8, forced: ForcedField, value: i32) -> Result<(), String> {
    {
        let p = board.player_mut(idx)?;
        match forced {
            ForcedField::Strength => p.strength_extra = 0,
            ForcedField::Happy => p.happy_extra = 0,
            ForcedField::CultureRate => p.culture_rate_extra = 0,
            ForcedField::ScienceRate => p.science_rate_extra = 0,
        }
    }
    let base = {
        let p = board.player(idx)?;
        let stats = effects::compute(&board.state, p);
        match forced {
            ForcedField::Strength => stats.strength,
            ForcedField::Happy => stats.happy,
            ForcedField::CultureRate => stats.culture,
            ForcedField::ScienceRate => stats.science,
        }
    };
    let extra = (value - base) as i16;
    let p = board.player_mut(idx)?;
    match forced {
        ForcedField::Strength => p.strength_extra = extra,
        ForcedField::Happy => p.happy_extra = extra,
        ForcedField::CultureRate => p.culture_rate_extra = extra,
        ForcedField::ScienceRate => p.science_rate_extra = extra,
    }
    Ok(())
}

// ---------------------------------------------------------------- patches

pub const PATCH_HELP: &str = "\
between-turn updates (one per line, blank line when done):

  deal <cards>          new cards dealt on the right after the sweep
  row <13 cards>        retype the whole row ('.' = empty slot)
  take p1 7             a rival took the card in row slot 7 (0-based)
  p1 c=34 s=9 str=6     a rival's scalars
  p1 tech+ Bronze:2      add/replace a technology (name:workers)
  p1 tech- Warriors      remove a technology
  p1 wonder pyr 2        wonder in progress + steps built
  p1 built+ Colossus     completed a wonder (by NAME -- there is no w= count)
  p1 colony+ Vast Territory (I)   took a colony (by NAME; face up)
  p1 leader Caesar       played a leader ('-' clears)
  p1 tactic Legion       played a tactic
  p1 gov=Monarchy        changed government
  p1 hc=3 hm=2           rival hand sizes
  event <card>           a new current event was revealed
  age II                 the age advanced
  ?                      anything you don't know -- e.g. p1 c=?
";

/// A parsed one-liner. Mirrors `_patch`'s `word` dispatch, made into a type
/// instead of a chain of string comparisons re-run at every call site.
enum Command {
    Blank,
    Deal(Vec<CardId>),
    Row(Vec<CardId>),
    Take { player: u8, slot: u8 },
    Event(Vec<CardId>),
    Age(Age),
    LastRound,
    Current(u8),
    Player { idx: u8, verb: PlayerVerb },
}

enum ColonyEdit {
    Clear,
    Add(Vec<CardId>),
}

enum WonderEdit {
    Clear,
    Set { card: CardId, steps: u8 },
}

/// The verbs `_patch_player` understands that are NOT the shared snapshot
/// grammar (`PlayerVerb::Delegate` is that grammar, reused as-is).
enum PlayerVerb {
    TechAdd(Vec<(CardId, u8)>),
    /// Resolved ids to remove, plus the raw text for the confirmation
    /// message (Python echoes what was typed, not what it resolved to).
    TechRemove { ids: Vec<CardId>, raw: String },
    BuiltAdd { ids: Vec<CardId>, raw: String },
    ColonyAdd(ColonyEdit),
    ColonyRemove(Vec<CardId>),
    Wonder(WonderEdit),
    Delegate(PlayerLine),
    Scalars(Vec<PlayerToken>),
}

/// Apply ONE human-typed update line. Returns a short confirmation, or an
/// error the caller can print and re-prompt on -- never anything else.
pub fn patch(board: &mut Board, line: &str) -> Result<String, String> {
    let cmd = parse_command(board, line)?;
    apply_command(board, cmd)
}

fn parse_command(board: &Board, line: &str) -> Result<Command, String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return Ok(Command::Blank);
    }
    let (word_raw, rest) = split_first(line);
    let word = word_raw.to_ascii_lowercase();
    if let Some(idx) = parse_player_word(&word) {
        return Ok(Command::Player { idx, verb: parse_player_verb(board, idx, rest)? });
    }
    match word.as_str() {
        "deal" => {
            let names = resolve_names(&split_cards(rest, Some(Pool::Row)), Pool::Row)?;
            Ok(Command::Deal(names))
        }
        "row" => {
            let toks = split_cards(rest, Some(Pool::Row));
            if toks.is_empty() {
                return Err("usage: row <up to 13 cards, '.' for empty>".to_string());
            }
            Ok(Command::Row(resolve_row_tokens(&toks)?))
        }
        "take" => {
            let bits: Vec<&str> = rest.split_whitespace().collect();
            if bits.len() != 2 {
                return Err("usage: take p<N> <slot 0-12>".to_string());
            }
            let player = parse_player_idx(bits[0])?;
            let slot: usize = parse_num(bits[1], "slot")?;
            if slot >= ROW_SIZE {
                return Err(format!("row slot must be 0..{}", ROW_SIZE - 1));
            }
            Ok(Command::Take { player, slot: slot as u8 })
        }
        "event" => {
            let names = resolve_names(&split_cards(rest, Some(Pool::Military)), Pool::Military)?;
            Ok(Command::Event(names))
        }
        "age" => Ok(Command::Age(parse_age(&rest.trim().to_ascii_uppercase())?)),
        "last" | "last_round" => Ok(Command::LastRound),
        "turn" | "cur" | "current" => Ok(Command::Current(parse_player_idx(rest)?)),
        _ => Err(format!("don't understand {word:?}.\n{PATCH_HELP}")),
    }
}

fn current_tech_ids(board: &Board, idx: u8) -> Result<Vec<CardId>, String> {
    Ok(board.player(idx)?.techs.iter().map(|(id, _)| id).collect())
}

fn current_colony_ids(board: &Board, idx: u8) -> Result<Vec<CardId>, String> {
    Ok(board.player(idx)?.colonies.as_slice().to_vec())
}

fn parse_player_verb(board: &Board, idx: u8, rest: &str) -> Result<PlayerVerb, String> {
    let (head_raw, tail) = split_first(rest);
    let head = head_raw.to_ascii_lowercase();
    match head.as_str() {
        "tech+" | "+tech" => {
            let tokens = split_cards(tail, Some(Pool::Tech));
            Ok(PlayerVerb::TechAdd(parse_tech_tokens(&tokens, "1")?))
        }
        "tech-" | "-tech" => {
            let extra = current_tech_ids(board, idx)?;
            let mut ids = Vec::new();
            for tok in split_cards(tail, Some(Pool::Tech)) {
                ids.push(resolve_card(&tok, Pool::Tech, &extra)?);
            }
            Ok(PlayerVerb::TechRemove { ids, raw: tail.to_string() })
        }
        "built+" | "+built" | "built" => {
            let tokens = split_cards(tail, Some(Pool::Wonder));
            let ids = resolve_names(&tokens, Pool::Wonder)?;
            Ok(PlayerVerb::BuiltAdd { ids, raw: tail.to_string() })
        }
        "colony+" | "+colony" | "colony" => {
            if is_clear_marker(tail) {
                Ok(PlayerVerb::ColonyAdd(ColonyEdit::Clear))
            } else {
                let tokens = split_cards(tail, Some(Pool::Territory));
                Ok(PlayerVerb::ColonyAdd(ColonyEdit::Add(resolve_names(&tokens, Pool::Territory)?)))
            }
        }
        "colony-" | "-colony" => {
            let extra = current_colony_ids(board, idx)?;
            let mut ids = Vec::new();
            for tok in split_cards(tail, Some(Pool::Territory)) {
                ids.push(resolve_card(&tok, Pool::Territory, &extra)?);
            }
            Ok(PlayerVerb::ColonyRemove(ids))
        }
        "wonder" | "wonder+" => {
            if is_clear_marker(tail) {
                Ok(PlayerVerb::Wonder(WonderEdit::Clear))
            } else {
                let (name, steps) = parse_wonder_tail(tail);
                let card = resolve_card(&name, Pool::Wonder, &[])?;
                Ok(PlayerVerb::Wonder(WonderEdit::Set { card, steps }))
            }
        }
        "leader" | "tactic" | "hand" | "hidden" | "gov" | "tech" => {
            Ok(PlayerVerb::Delegate(parse_player_line(rest)?))
        }
        _ => Ok(PlayerVerb::Scalars(parse_player_tokens(rest)?)),
    }
}

fn apply_command(board: &mut Board, cmd: Command) -> Result<String, String> {
    match cmd {
        Command::Blank => Ok(String::new()),
        Command::Deal(names) => {
            let n = names.len();
            advance_row(board, &names, None)?;
            Ok(format!("row advanced, dealt {n} card(s)"))
        }
        Command::Row(names) => {
            sync_row(board, &names);
            Ok("row set".to_string())
        }
        Command::Take { player, slot } => {
            board.player(player)?;
            let name = board.state.card_row[slot as usize];
            if name.is_none() {
                return Err(format!("row slot {slot} is already empty"));
            }
            board.state.card_row[slot as usize] = CardId::NONE;
            // Counted, not named: we know a card was taken, but not what
            // ends up PLAYED, so a named hand could only ever grow.
            let cur = board.hidden_count(player, Hand::Civil)?;
            board.set_hidden(player, Hand::Civil, cur as i64 + 1)?;
            Ok(format!("p{player} took {} from slot {slot}", name.name()))
        }
        Command::Event(names) => {
            let n = names.len();
            for name in names {
                board.state.current_events.push(name);
            }
            let _ = n;
            Ok("current event(s) noted".to_string())
        }
        Command::Age(age) => {
            board.state.age_civil = age;
            board.state.age_military = age;
            Ok(format!("age -> {}", age_str(age)))
        }
        Command::LastRound => {
            board.state.last_round = true;
            Ok("final round".to_string())
        }
        Command::Current(p) => {
            board.player(p)?;
            board.state.current = p;
            Ok(format!("current player = p{p}"))
        }
        Command::Player { idx, verb } => apply_player_verb(board, idx, verb),
    }
}

fn apply_player_verb(board: &mut Board, idx: u8, verb: PlayerVerb) -> Result<String, String> {
    board.player(idx)?;
    match verb {
        PlayerVerb::TechAdd(list) => {
            let mut confirmed = Vec::new();
            {
                let p = board.player_mut(idx)?;
                for (id, w) in &list {
                    p.techs.remove(*id);
                    p.techs.insert(*id, TechSlot { workers: *w, stored: 0 });
                    confirmed.push(format!("{}:{w}", id.name()));
                }
            }
            Ok(format!("p{idx} techs {}", confirmed.join(", ")))
        }
        PlayerVerb::TechRemove { ids, raw } => {
            let p = board.player_mut(idx)?;
            for id in ids {
                p.techs.remove(id);
            }
            Ok(format!("p{idx} removed {raw}"))
        }
        PlayerVerb::BuiltAdd { ids, raw } => {
            let p = board.player_mut(idx)?;
            for id in ids {
                if !p.completed_wonders.contains(id) {
                    p.completed_wonders.push(id);
                }
                if p.wonder == id {
                    p.wonder = CardId::NONE;
                    p.wonder_steps = 0;
                }
            }
            Ok(format!("p{idx} completed {raw}"))
        }
        PlayerVerb::ColonyAdd(edit) => {
            let p = board.player_mut(idx)?;
            match edit {
                ColonyEdit::Clear => {
                    p.colonies = CardList::new();
                    Ok(format!("p{idx} has no colonies"))
                }
                ColonyEdit::Add(ids) => {
                    for id in ids {
                        if !p.colonies.contains(id) {
                            p.colonies.push(id);
                        }
                    }
                    Ok(format!("p{idx} colonies {}", join_names(p.colonies.as_slice())))
                }
            }
        }
        PlayerVerb::ColonyRemove(ids) => {
            let p = board.player_mut(idx)?;
            for id in ids {
                p.colonies.remove_first(id);
            }
            let names = join_names(p.colonies.as_slice());
            Ok(format!("p{idx} colonies {}", if names.is_empty() { "(none)".to_string() } else { names }))
        }
        PlayerVerb::Wonder(edit) => match edit {
            WonderEdit::Clear => {
                let p = board.player_mut(idx)?;
                p.wonder = CardId::NONE;
                p.wonder_steps = 0;
                Ok(format!("p{idx} has no wonder in progress"))
            }
            WonderEdit::Set { card, steps } => {
                let p = board.player_mut(idx)?;
                p.wonder = card;
                p.wonder_steps = steps;
                Ok(format!("p{idx} wonder {} {steps} step(s)", card.name()))
            }
        },
        PlayerVerb::Delegate(line) => {
            let head = line.head_word();
            apply_player_line(board, idx, line)?;
            Ok(format!("p{idx} {head} updated"))
        }
        PlayerVerb::Scalars(tokens) => {
            let msgs = apply_player_tokens(board, idx, tokens)?;
            if msgs.is_empty() {
                Ok(format!("p{idx} updated"))
            } else {
                Ok(msgs)
            }
        }
    }
}

/// Apply a whole block of update lines; returns `(confirmations, errors)`.
pub fn patch_all(board: &mut Board, text: &str) -> (Vec<String>, Vec<String>) {
    let mut msgs = Vec::new();
    let mut errs = Vec::new();
    for ln in text.lines() {
        match patch(board, ln) {
            Ok(m) => {
                if !m.is_empty() {
                    msgs.push(m);
                }
            }
            Err(e) => errs.push(format!("{:?}: {e}", ln.trim())),
        }
    }
    (msgs, errs)
}

// ------------------------------------------------------------ pretty print

fn type_label(kind: CardType) -> &'static str {
    use CardType::*;
    match kind {
        Farm => "farm",
        Mine => "mine",
        Lab => "lab",
        Temple => "temple",
        Library => "library",
        Arena => "arena",
        Theater => "theater",
        Infantry => "infantry",
        Cavalry => "cavalry",
        Artillery => "artillery",
        Air => "air",
        Government => "government",
        SpecialTech => "special-tech",
        Wonder => "wonder",
        Leader => "leader",
        Action => "action",
        Tactic => "tactic",
        Aggression => "aggression",
        War => "war",
        Pact => "pact",
        Bonus => "bonus",
        Territory => "territory",
        Event => "event",
    }
}

/// A board summary a human can check against the table in one glance.
pub fn render(board: &Board, width: usize) -> String {
    let st = &board.state;
    let mut l = Vec::new();
    l.push(format!(
        "== TTA  round {}  age {}  turn: p{}{}{}",
        st.round,
        age_str(st.age_civil),
        st.current,
        if st.current == board.me { "  (you)" } else { "" },
        if st.last_round { "  [FINAL ROUND]" } else { "" },
    ));
    l.push("-".repeat(width));
    l.push("card row (cost 1 / 1 1 1 1 | 2 2 2 2 | 3 3 3 3):".to_string());
    for (i, &c) in st.card_row.iter().enumerate() {
        let cost = if i < 5 { 1 } else if i < 9 { 2 } else { 3 };
        if c.is_none() {
            l.push(format!("  {i:>2} ({cost}) --"));
        } else {
            l.push(format!(
                "  {i:>2} ({cost}) {}  [{} {}]",
                c.name(),
                type_label(c.kind()),
                age_str(c.get().age)
            ));
        }
    }
    if !st.current_events.is_empty() {
        l.push(format!(
            "current events: {}   (future {})",
            join_names(st.current_events.as_slice()),
            st.future_events.len()
        ));
    }
    if !st.available_tactics.is_empty() {
        l.push(format!("tactics available: {}", join_names(st.available_tactics.as_slice())));
    }
    l.push("-".repeat(width));
    for p in st.players[..st.num_players as usize].iter() {
        render_player(&mut l, board, p);
    }
    if !board.unknown.is_empty() {
        let names: Vec<String> = board
            .unknown
            .iter()
            .map(|u| format!("p{}.{}", u.player, u.field.short_name()))
            .collect();
        l.push(format!("unknown: {}", names.join(", ")));
    }
    l.join("\n")
}

fn render_player(l: &mut Vec<String>, board: &Board, p: &PlayerState) {
    let s = effects::compute(&board.state, p);
    let need = economy::happy_required(p.yellow_bank);
    let who = if p.idx == board.me { format!("p{} (you)", p.idx) } else { format!("p{}", p.idx) };
    let leader = if p.leader.is_none() { String::new() } else { format!("  leader: {}", p.leader.name()) };
    let tactic = if p.tactic.is_none() { String::new() } else { format!("  tactic: {}", p.tactic.name()) };
    l.push(format!("{who}  {}{leader}{tactic}", p.government.name()));
    l.push(format!(
        "   culture {} (+{}/t)   science {} (+{}/t)   strength {}   happy {}/{need}",
        p.culture, s.culture, p.science, s.science, s.strength, s.happy
    ));
    l.push(format!(
        "   food {} (+{})   res {} (+{})   CA {}/{}   MA {}/{}   free workers {}   yellow bank {}",
        p.food, s.food, p.resources, s.resources, p.civil_actions, s.civil_actions,
        p.military_actions, s.military_actions, p.workers_free, p.yellow_bank
    ));
    let mut techs: Vec<(&str, u8, CardType)> =
        p.techs.iter().map(|(id, slot)| (id.name(), slot.workers, id.kind())).collect();
    techs.sort_unstable_by_key(|&(n, _, _)| n);
    let techs_str = techs
        .iter()
        .filter(|&&(_, w, kind)| w > 0 || kind != CardType::SpecialTech)
        .map(|&(n, w, _)| format!("{n}:{w}"))
        .collect::<Vec<_>>()
        .join(", ");
    l.push(format!("   techs: {techs_str}"));
    if !p.wonder.is_none() {
        let stages = p.wonder.get().stages;
        let done = (p.wonder_steps as usize).min(stages.len());
        let left: Vec<String> = stages[done..].iter().map(u8::to_string).collect();
        l.push(format!(
            "   building: {} {done}/{}  (remaining stages {})",
            p.wonder.name(),
            stages.len(),
            left.join(",")
        ));
    }
    if !p.completed_wonders.is_empty() {
        l.push(format!("   wonders: {}", join_names(p.completed_wonders.as_slice())));
    }
    if p.idx == board.me && (!p.hand_civil.is_empty() || !p.hand_military.is_empty()) {
        l.push(format!("   hand civil: {}", fmt_hand_sorted(p.hand_civil.as_slice())));
        l.push(format!("   hand mil:   {}", fmt_hand_sorted(p.hand_military.as_slice())));
    } else {
        let hc = p.hand_civil.len() + p.hidden_civil as usize;
        let hm = p.hand_military.len() + p.hidden_military as usize;
        l.push(format!("   hand: {hc} civil, {hm} military"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bots::greedy::GreedyBot;

    // ------------------------------------------------------ card resolution

    #[test]
    fn exact_name_resolves_to_itself() {
        assert_eq!(resolve_card("Bronze", Pool::Any, &[]).unwrap().name(), "Bronze");
    }

    #[test]
    fn case_and_punctuation_are_ignored() {
        assert_eq!(resolve_card("bronze", Pool::Any, &[]).unwrap().name(), "Bronze");
    }

    #[test]
    fn a_prefix_resolves_within_its_pool() {
        assert_eq!(resolve_card("philos", Pool::Tech, &[]).unwrap().name(), "Philosophy");
    }

    #[test]
    fn initials_resolve_a_multiword_name() {
        assert_eq!(resolve_card("hg", Pool::Wonder, &[]).unwrap().name(), "Hanging Gardens");
    }

    #[test]
    fn a_subsequence_resolves_when_nothing_earlier_matches() {
        assert_eq!(resolve_card("hanggard", Pool::Wonder, &[]).unwrap().name(), "Hanging Gardens");
    }

    #[test]
    fn a_narrow_pool_resolves_what_the_wide_pool_would_leave_ambiguous() {
        assert_eq!(resolve_card("julius", Pool::Leader, &[]).unwrap().name(), "Julius Caesar");
    }

    #[test]
    fn an_ambiguous_query_lists_every_candidate() {
        let err = resolve_card("a", Pool::Leader, &[]).unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
    }

    #[test]
    fn an_unmatched_query_is_an_error_not_a_panic() {
        assert!(resolve_card("zzzzzz", Pool::Any, &[]).is_err());
    }

    // --------------------------------------------------------- round trips

    fn fresh_boards() -> Vec<Board> {
        (2u8..=4).map(|n| new_board(n, 0, n as u64)).collect()
    }

    #[test]
    fn a_freshly_dealt_game_survives_a_round_trip_unchanged() {
        for b in fresh_boards() {
            let text = dumps(&b);
            let again = dumps(&loads(&text).unwrap());
            assert_eq!(text, again);
        }
    }

    /// Plays a real game forward with the greedy bot, then proves the text
    /// form round-trips -- not just a fresh deal, which never exercises
    /// hands, techs, wonders or hidden counts.
    #[test]
    fn a_midgame_snapshot_survives_a_round_trip_unchanged() {
        for seed in [1u64, 7] {
            let mut st = game::new_game(3, seed);
            let bots = [GreedyBot::default(); 3];
            for _ in 0..220 {
                if st.game_over {
                    break;
                }
                let moves = crate::legal::legal_moves(&st);
                let mv = bots[st.decider() as usize].choose(&st, moves.as_slice());
                crate::apply::apply(&mut st, mv);
            }
            let mut b = Board { state: st.clone(), me: 0, unknown: BTreeSet::new() };
            b.set_hidden(1, Hand::Civil, 2).unwrap();
            b.unknown.insert(UnknownField { player: 2, field: PlayerField::Culture });
            let text = dumps(&b);
            let b2 = loads(&text).unwrap();
            assert_eq!(text, dumps(&b2));

            // semantic spot checks
            assert_eq!(b2.state.round, st.round);
            assert_eq!(b2.state.card_row, st.card_row);
            assert_eq!(b2.state.players[0].culture, st.players[0].culture);
            let orig_techs: BTreeSet<CardId> = st.players[0].techs.iter().map(|(id, _)| id).collect();
            let new_techs: BTreeSet<CardId> = b2.state.players[0].techs.iter().map(|(id, _)| id).collect();
            assert_eq!(orig_techs, new_techs);
            assert_eq!(b2.hidden_count(1, Hand::Civil).unwrap(), 2);
            // the count is real STATE, not advisor-only bookkeeping.
            assert_eq!(
                b2.hand_size(1, Hand::Civil).unwrap(),
                b2.state.players[1].hand_civil.len() + 2
            );
            assert!(b2.unknown.contains(&UnknownField { player: 2, field: PlayerField::Culture }));
        }
    }

    #[test]
    fn hand_leader_and_wonders_survive_a_round_trip() {
        let mut b = new_board(2, 0, 3);
        {
            let p = b.player_mut(0).unwrap();
            p.hand_civil = fill_cardlist(&[
                CardId::by_name("Bronze").unwrap(),
                CardId::by_name("Irrigation").unwrap(),
            ]);
            p.leader = CardId::by_name("Julius Caesar").unwrap();
            p.wonder = CardId::NONE;
        }
        patch(&mut b, "p0 wonder Pyramids 2").unwrap();
        patch(&mut b, "p0 built+ Colossus").unwrap();
        let b2 = loads(&dumps(&b)).unwrap();
        let q = &b2.state.players[0];
        let mut hand: Vec<&str> = q.hand_civil.as_slice().iter().map(|c| c.name()).collect();
        hand.sort_unstable();
        assert_eq!(hand, vec!["Bronze", "Irrigation"]);
        assert_eq!(q.leader.name(), "Julius Caesar");
        assert_eq!(q.wonder.name(), "Pyramids");
        assert_eq!(q.wonder_steps, 2);
        assert_eq!(q.completed_wonders.as_slice(), &[CardId::by_name("Colossus").unwrap()]);
    }

    // --------------------------------------------------------------- patch

    fn patch_board() -> Board {
        new_board(3, 0, 11)
    }

    #[test]
    fn scalar_keys_set_the_named_fields() {
        let mut b = patch_board();
        patch(&mut b, "p1 c=34 s=9 f=4 r=6").unwrap();
        let p = &b.state.players[1];
        assert_eq!((p.culture, p.science, p.food, p.resources), (34, 9, 4, 6));
    }

    #[test]
    fn a_forced_strength_backs_out_the_extra_so_compute_agrees() {
        let mut b = patch_board();
        patch(&mut b, "p1 str=11").unwrap();
        assert_eq!(effects::compute(&b.state, &b.state.players[1]).strength, 11);
    }

    #[test]
    fn an_unknown_value_leaves_the_mirror_and_records_the_field() {
        let mut b = patch_board();
        let before = b.state.players[1].culture;
        patch(&mut b, "p1 c=?").unwrap();
        assert_eq!(b.state.players[1].culture, before);
        assert!(b.unknown.contains(&UnknownField { player: 1, field: PlayerField::Culture }));
    }

    #[test]
    fn tech_plus_and_minus_add_and_remove() {
        let mut b = patch_board();
        patch(&mut b, "p1 tech+ irrigation:2").unwrap();
        assert_eq!(
            b.state.players[1].techs.get(CardId::by_name("Irrigation").unwrap()).unwrap().workers,
            2
        );
        patch(&mut b, "p1 tech- warriors").unwrap();
        assert!(!b.state.players[1].techs.has(CardId::by_name("Warriors").unwrap()));
    }

    #[test]
    fn take_clears_the_row_slot_and_counts_the_hand() {
        let mut b = patch_board();
        let name = b.state.card_row[4];
        patch(&mut b, "take p2 4").unwrap();
        assert!(b.state.card_row[4].is_none());
        assert_eq!(b.hand_size(2, Hand::Civil).unwrap(), 1);
        assert!(!name.is_none());
    }

    #[test]
    fn deal_sweeps_the_row_and_appends_new_cards() {
        let mut b = patch_board();
        let old = b.state.card_row;
        patch(&mut b, "deal bronze, irrigation").unwrap();
        let row = b.state.card_row;
        assert_eq!(row[0], old[2], "3p sweep is 2");
        assert!(row.contains(&CardId::by_name("Bronze").unwrap()));
        assert!(row.contains(&CardId::by_name("Irrigation").unwrap()));
        assert_eq!(row.len(), ROW_SIZE);
    }

    #[test]
    fn row_retypes_the_whole_row_with_dots_as_empty_slots() {
        let mut b = patch_board();
        patch(&mut b, "row bronze, ., philosophy").unwrap();
        assert_eq!(b.state.card_row[0], CardId::by_name("Bronze").unwrap());
        assert!(b.state.card_row[1].is_none());
        assert_eq!(b.state.card_row[2], CardId::by_name("Philosophy").unwrap());
        assert_eq!(b.state.card_row.len(), ROW_SIZE);
    }

    #[test]
    fn government_and_leader_update_and_leader_clears() {
        let mut b = patch_board();
        patch(&mut b, "p1 gov=monarchy").unwrap();
        assert_eq!(b.state.players[1].government.name(), "Monarchy");
        patch(&mut b, "p1 leader caesar").unwrap();
        assert_eq!(b.state.players[1].leader.name(), "Julius Caesar");
        patch(&mut b, "p1 leader -").unwrap();
        assert!(b.state.players[1].leader.is_none());
    }

    #[test]
    fn hc_and_hm_set_the_rivals_total_hand_size() {
        let mut b = patch_board();
        patch(&mut b, "p1 hc=4 hm=2").unwrap();
        assert_eq!(b.hand_size(1, Hand::Civil).unwrap(), 4);
        assert_eq!(b.hand_size(1, Hand::Military).unwrap(), 2);
    }

    #[test]
    fn bad_input_is_an_error_never_a_panic() {
        let mut b = patch_board();
        for bad in [
            "zzz", "p1 c=x", "p9 c=1", "take p1", "take p1 99", "p1 tech+ notacard",
            "p1 nosuchkey=3", "age Z", "p1 gov=zzzz", "row",
        ] {
            assert!(patch(&mut b, bad).is_err(), "{bad:?} should have failed");
        }
    }

    #[test]
    fn blank_and_comment_lines_are_noops() {
        let mut b = patch_board();
        assert_eq!(patch(&mut b, "").unwrap(), "");
        assert_eq!(patch(&mut b, "  # note").unwrap(), "");
    }

    #[test]
    fn patch_all_collects_successes_and_errors_separately() {
        let mut b = patch_board();
        let (msgs, errs) = patch_all(&mut b, "p1 c=5\nnonsense\np2 s=3");
        assert_eq!(msgs.len(), 2);
        assert_eq!(errs.len(), 1);
    }

    #[test]
    fn render_does_not_panic_and_shows_the_row_and_seat() {
        let b = patch_board();
        let txt = render(&b, 78);
        assert!(txt.contains("card row"));
        assert!(txt.contains("p0"));
    }

    #[test]
    fn a_patched_mirror_stays_a_legal_engine_state() {
        let mut b = patch_board();
        patch(&mut b, "p1 c=34 str=7").unwrap();
        patch(&mut b, "p1 tech+ irrigation:2").unwrap();
        patch(&mut b, "deal bronze, irrigation").unwrap();
        let moves = crate::legal::legal_moves(&b.state);
        assert!(!moves.as_slice().is_empty());
        let mv = *moves.as_slice().last().unwrap();
        crate::apply::apply(&mut b.state, mv);
    }

    #[test]
    fn colony_edits_add_clear_and_remove() {
        let mut b = patch_board();
        let id = CardId::by_name("Vast Territory (I)").unwrap();
        patch(&mut b, "p1 colony+ Vast Territory (I)").unwrap();
        assert!(b.state.players[1].colonies.contains(id));
        patch(&mut b, "p1 colony- Vast Territory (I)").unwrap();
        assert!(!b.state.players[1].colonies.contains(id));
        patch(&mut b, "p1 colony+ Vast Territory (I)").unwrap();
        patch(&mut b, "p1 colony+ -").unwrap();
        assert!(b.state.players[1].colonies.is_empty());
    }

    #[test]
    fn wonder_can_be_set_and_cleared_through_patch() {
        let mut b = patch_board();
        patch(&mut b, "p1 wonder pyramids 2").unwrap();
        assert_eq!(b.state.players[1].wonder.name(), "Pyramids");
        assert_eq!(b.state.players[1].wonder_steps, 2);
        patch(&mut b, "p1 wonder -").unwrap();
        assert!(b.state.players[1].wonder.is_none());
    }
}
