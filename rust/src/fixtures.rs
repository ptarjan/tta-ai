//! A minimal, dependency-free JSON value parser.
//!
//! This file used to be much larger: it was the reader for the
//! differential-testing fixtures `tools/dump_fixtures.py` (and its sibling
//! `dump_*.py` scripts) wrote -- recordings of the Python engine's answers
//! that `rust/tests/` replayed Rust against to certify the port. That corpus
//! and everything that only existed to read it (`Header`/`Ply`/`Footer`/
//! `Record`, `parse_move`, `parse_record`, `read_fixture_file`,
//! `fixture_files`, `GameState::from_json`, `GameState::diff`) were retired
//! together with the Python engine: once Python is gone the fixtures can
//! never be regenerated, so keeping Rust passing against them would freeze
//! this port against a dead engine's bugs forever rather than against the
//! rulebook. See the commit that removed them for the full accounting.
//!
//! What's left is genuinely still needed: [`Json`]/[`parse_json`] is a
//! general-purpose JSON value reader with no `[dependencies]` entry
//! (`serde_json` would be fine in `[dev-dependencies]`, but this has real
//! non-test callers too -- `bots::weighted::eval::parse_weights` reads a
//! trained champion's weight file with it, and `bin/climb.rs` reads a
//! checkpoint's training-progress fields the same way). Neither of those
//! needs a full JSON implementation: this handles everything `json.dumps`
//! can produce, and nothing it cannot.

use std::fmt;

// ============================================================ json ======

/// A parsed JSON value. Objects keep insertion order (a `Vec` of pairs, not a
/// map) because nothing here needs key lookup to be fast -- these files are
/// small and read once, not on a hot path -- and because that is also
/// simpler than pulling in a hasher.
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    /// All JSON numbers, integer or not. `serde_json` would keep an int/float
    /// split; this reader doesn't need it, since every numeric field here
    /// (seeds, plies, slots, card ages) is small enough to round-trip exactly
    /// through `f64`.
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Json::Null | Json::Bool(_) | Json::Num(_) | Json::Str(_) | Json::Arr(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            Json::Null | Json::Bool(_) | Json::Num(_) | Json::Arr(_) | Json::Obj(_) => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            Json::Null | Json::Bool(_) | Json::Str(_) | Json::Arr(_) | Json::Obj(_) => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            Json::Null | Json::Num(_) | Json::Str(_) | Json::Arr(_) | Json::Obj(_) => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            Json::Null | Json::Bool(_) | Json::Num(_) | Json::Str(_) | Json::Obj(_) => None,
        }
    }

    /// Build an object from `(key, value)` pairs, in the order given --
    /// `Json::Obj` keeps insertion order (see this type's doc comment), so
    /// that order is exactly what [`Json::to_string`] prints. Callers that
    /// want `harness::record`'s JSONL to read like the Python original's
    /// dataclass-field order should pass pairs in that order.
    pub fn obj(fields: Vec<(&str, Json)>) -> Json {
        Json::Obj(fields.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    /// The write side of this module: turn a [`Json`] back into text.
    /// [`harness::record`]'s append-only game log is the reason this exists
    /// -- `serde_json` is off the table (`Cargo.toml`'s `[dependencies]` is
    /// deliberately empty), and a JSONL writer needs SOME serializer, so this
    /// reader grows the matching writer rather than a second, parallel one
    /// living in `harness` alone.
    fn write(&self, out: &mut String) {
        use std::fmt::Write as _;
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                // Every number this codebase writes (seeds, plies, scores,
                // rounded scores) is either a whole number or was rounded to
                // a fixed number of decimals before it got here -- so
                // printing an integer as an integer (not "41.0") is the
                // right default, and anything with a fractional part falls
                // through to Rust's own shortest round-tripping `f64`
                // formatting, which `f64::from_str` reads back exactly.
                if n.is_finite() && n.fract() == 0.0 && n.abs() < 1e15 {
                    write!(out, "{}", *n as i64).unwrap();
                } else {
                    write!(out, "{n}").unwrap();
                }
            }
            Json::Str(s) => write_json_string(out, s),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

// clippy::inherent_to_string wants a real `Display` impl instead of an
// inherent `to_string` (the two aren't equivalent to the compiler: an
// inherent method silently shadows the blanket `ToString` impl a `Display`
// impl would give for free). This preserves every existing `.to_string()`
// call site byte-for-byte -- same `write` helper, same output -- while
// making `Json` a normal `Display` type too.
impl std::fmt::Display for Json {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = String::new();
        self.write(&mut out);
        f.write_str(&out)
    }
}

fn write_json_string(out: &mut String, s: &str) {
    use std::fmt::Write as _;
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[derive(Debug)]
struct JsonError {
    pos: usize,
    msg: String,
}

struct JsonParser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn new(s: &'a str) -> Self {
        JsonParser { s: s.as_bytes(), pos: 0 }
    }

    fn err(&self, msg: impl Into<String>) -> JsonError {
        JsonError { pos: self.pos, msg: msg.into() }
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, b: u8) -> Result<(), JsonError> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err(self.err(format!("expected {:?}", b as char)))
        }
    }

    fn literal(&mut self, lit: &str, val: Json) -> Result<Json, JsonError> {
        let end = self.pos + lit.len();
        if self.s.get(self.pos..end) == Some(lit.as_bytes()) {
            self.pos = end;
            Ok(val)
        } else {
            Err(self.err(format!("expected {lit:?}")))
        }
    }

    fn parse_value(&mut self) -> Result<Json, JsonError> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(Json::Str),
            Some(b't') => self.literal("true", Json::Bool(true)),
            Some(b'f') => self.literal("false", Json::Bool(false)),
            Some(b'n') => self.literal("null", Json::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            Some(c) => Err(self.err(format!("unexpected byte {:?}", c as char))),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn parse_object(&mut self) -> Result<Json, JsonError> {
        self.expect(b'{')?;
        let mut fields = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Json::Obj(fields));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let val = self.parse_value()?;
            fields.push((key, val));
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b'}') => break,
                _ => return Err(self.err("expected ',' or '}' in object")),
            }
        }
        Ok(Json::Obj(fields))
    }

    fn parse_array(&mut self) -> Result<Json, JsonError> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            let val = self.parse_value()?;
            items.push(val);
            self.skip_ws();
            match self.bump() {
                Some(b',') => continue,
                Some(b']') => break,
                _ => return Err(self.err("expected ',' or ']' in array")),
            }
        }
        Ok(Json::Arr(items))
    }

    fn parse_string(&mut self) -> Result<String, JsonError> {
        self.expect(b'"')?;
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => return Ok(out),
                Some(b'\\') => match self.bump() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'b') => out.push('\u{0008}'),
                    Some(b'f') => out.push('\u{000C}'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'u') => {
                        let cp = self.hex4()?;
                        // No surrogate-pair handling: nothing in this
                        // dataset (card names, English prose) needs it, and
                        // the fixture producer never emits one.
                        out.push(char::from_u32(cp as u32).unwrap_or('\u{FFFD}'));
                    }
                    _ => return Err(self.err("bad escape")),
                },
                Some(b) => {
                    // Reinterpret the remaining UTF-8 bytes of this codepoint
                    // directly from the source rather than decoding by hand.
                    let start = self.pos - 1;
                    let width = utf8_width(b);
                    let end = start + width;
                    let bytes = self.s.get(start..end).ok_or_else(|| self.err("truncated utf8"))?;
                    let s = std::str::from_utf8(bytes).map_err(|_| self.err("invalid utf8"))?;
                    out.push_str(s);
                    self.pos = end;
                }
            }
        }
    }

    fn hex4(&mut self) -> Result<u16, JsonError> {
        let mut v: u16 = 0;
        for _ in 0..4 {
            let c = self.bump().ok_or_else(|| self.err("truncated \\u escape"))?;
            let d = (c as char).to_digit(16).ok_or_else(|| self.err("bad hex digit"))?;
            v = v * 16 + d as u16;
        }
        Ok(v)
    }

    fn parse_number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                self.pos += 1;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).unwrap();
        text.parse::<f64>().map(Json::Num).map_err(|_| self.err("bad number"))
    }
}

fn utf8_width(first_byte: u8) -> usize {
    if first_byte & 0x80 == 0 {
        1
    } else if first_byte & 0xE0 == 0xC0 {
        2
    } else if first_byte & 0xF0 == 0xE0 {
        3
    } else {
        4
    }
}

/// Parse a standalone JSON value. `pub` so both real callers --
/// `bots::weighted::eval::parse_weights` (a trained champion's weight file)
/// and `bin/climb.rs` (a checkpoint's training-progress fields) -- can use
/// the same reader without either owning a copy of it.
pub fn parse_json(s: &str) -> Result<Json, FixtureError> {
    let mut p = JsonParser::new(s);
    let v = p.parse_value().map_err(FixtureError::from)?;
    p.skip_ws();
    if p.pos != p.s.len() {
        return Err(FixtureError::from(p.err("trailing data after JSON value")));
    }
    Ok(v)
}

// =========================================================== errors ======

#[derive(Debug)]
pub struct FixtureError {
    pub message: String,
}

impl FixtureError {
    fn new(msg: impl Into<String>) -> Self {
        FixtureError { message: msg.into() }
    }
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for FixtureError {}

impl From<JsonError> for FixtureError {
    fn from(e: JsonError) -> Self {
        FixtureError::new(format!("json parse error at byte {}: {}", e.pos, e.msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_json_value_shape() {
        let v = parse_json(r#"{"a": 1, "b": [true, false, null, "x\ny"], "c": -1.5e2}"#).unwrap();
        assert_eq!(v.get("a").and_then(Json::as_f64), Some(1.0));
        let b = v.get("b").and_then(Json::as_arr).unwrap();
        assert_eq!(b[0].as_bool(), Some(true));
        assert_eq!(b[1].as_bool(), Some(false));
        assert_eq!(b[2], Json::Null);
        assert_eq!(b[3].as_str(), Some("x\ny"));
        assert_eq!(v.get("c").and_then(Json::as_f64), Some(-150.0));
    }

    #[test]
    fn rejects_trailing_garbage() {
        assert!(parse_json("1 2").is_err());
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(parse_json("{").is_err());
        assert!(parse_json("[1,]").is_err());
        assert!(parse_json("nul").is_err());
    }

    /// [`harness::record`]'s whole append-only log rests on this round-trip:
    /// what [`Json::to_string`] writes, [`parse_json`] must read back
    /// unchanged in every way that matters (object key order is the one
    /// exception `Json::Obj`'s own equality does not preserve, since a
    /// `Vec<(String, Json)>` compares positionally).
    #[test]
    fn writing_then_parsing_a_json_value_round_trips() {
        let v = Json::obj(vec![
            ("a", Json::Num(41.0)),
            ("b", Json::Str("quote \" and \\ and \nnewline".to_string())),
            ("c", Json::Arr(vec![Json::Bool(true), Json::Bool(false), Json::Null])),
            ("d", Json::Num(-1.5)),
        ]);
        let text = v.to_string();
        assert_eq!(parse_json(&text).unwrap(), v);
        // Whole numbers print bare, the way `json.dumps` prints a Python
        // int -- a re-scored log comparing `"c": 41` against `"c": 41.0`
        // should never see a difference that is only Rust's formatting.
        assert!(text.contains("\"a\":41"), "{text}");
    }
}
