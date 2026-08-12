//! The self-play data dump: one record per decision point, written by
//! `bin/dump_selfplay.rs` and read back by whatever eventually trains the
//! policy head.
//!
//! ## Why a new format, not [`super::rankdata::Shard`]
//!
//! `rankdata::Shard` already dumps Rust self-play, but for the VALUE net: a
//! `(chosen child encoding, rejected child encoding)` ranking pair, built by
//! applying each CANDIDATE move to a determinized copy of the state (its own
//! top doc comment's "hidden information in the labels" section is exactly
//! why that determinizing step exists). A policy head scores un-applied
//! actions in place -- [`super::action::encode_action`] takes no state at
//! all, precisely so it cannot leak a hypothetical future draw -- so there
//! is no child state to determinize here and nothing to share with that
//! format beyond the small binary-primitive helpers in `net.rs`
//! ([`super::net::Reader`]/`push_u32`/`push_f64_slice`, reused below rather
//! than re-invented a third time).
//!
//! ## On-disk format
//!
//! Little-endian throughout, `f64` (not `f32`) for every float -- this dump
//! is meant to be small (a policy head trains on far fewer rows than a value
//! net needs states), so there is no half-the-size incentive
//! `rankdata::Shard`'s own doc comment weighs for its `f32` choice.
//!
//! **Header** (written once, at the start of the file):
//! ```text
//! magic:       8 bytes, b"TTAPDMP1"
//! version:     u32
//! state_dim:   u32   -- width of every `state` block below
//! action_dim:  u32   -- width of every action block below
//! ```
//!
//! **Record** (repeated until EOF -- this is the "appendable" part: a
//! record carries no back-reference to the header and no file-level count
//! needs rewriting to add one, so a writer can always reopen an existing,
//! header-validated dump in append mode and push more records after
//! whatever is already there):
//! ```text
//! players:     u8    -- table size at this decision, 2..=4
//! actor:       u8    -- state.decider() -- whose turn this decision is
//! legal_count: u32
//! state:       state_dim  f64s  -- encode::encode(state, actor)
//! legal[0]:    action_dim f64s  -- action::encode_action(actor, legal_moves[0])
//! ...
//! legal[legal_count-1]: action_dim f64s
//! chosen:      u32   -- index into legal[] of the move actually played
//! result:      f64   -- actor's final win share (game::winners: 1.0 clean
//!                        win, 1/k for a k-way tie, 0.0 otherwise) -- backfilled
//!                        after the game ends, not known at record time
//! ```
//!
//! A reader that only wants a quick census (row count, dimension check) can
//! skip a record without decoding it: `legal_count` alone is enough to
//! compute its byte length ahead of the two trailing fixed fields.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;

use super::action::ACTION_DIM;
use super::encode::ENCODING_DIM;
use super::net::{push_f64_slice, push_u32, Reader};

const MAGIC: &[u8; 8] = b"TTAPDMP1";
const VERSION: u32 = 1;

/// One decision point: the public state, every legal action encoded, which
/// one was chosen, and the actor's eventual result. Plain data, not four
/// parallel `Vec`s kept in step by index -- see `rankdata.rs::Shard`'s own
/// doc comment for why this project avoids that shape.
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionRecord {
    pub players: u8,
    pub actor: u8,
    /// `encode::encode(state, actor)`'s output -- length [`ENCODING_DIM`].
    pub state: Vec<f64>,
    /// One [`super::action::encode_action`] output per legal move, in
    /// `legal_moves`'s own order -- length [`ACTION_DIM`] each.
    pub legal: Vec<Vec<f64>>,
    /// Index into `legal` of the move actually played.
    pub chosen: u32,
    /// The actor's final win share, backfilled once the game ends.
    pub result: f64,
}

fn header_mismatch(path: &Path, what: &str, got: u32, want: usize) -> String {
    format!(
        "{}: {what} {got} does not match this build's {what} {want} -- \
         regenerate this dump against the current encoders, do not read it as-is",
        path.display()
    )
}

/// Appends [`DecisionRecord`]s to a file in this module's format, writing
/// the header once when the file is freshly created.
pub struct DumpWriter {
    file: File,
}

impl DumpWriter {
    /// Create a brand new dump at `path`, truncating anything already
    /// there, and write the header.
    pub fn create(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
        }
        let mut file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut header = Vec::new();
        header.extend_from_slice(MAGIC);
        push_u32(&mut header, VERSION);
        push_u32(&mut header, ENCODING_DIM as u32);
        push_u32(&mut header, ACTION_DIM as u32);
        file.write_all(&header).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(DumpWriter { file })
    }

    /// Reopen an existing dump written by [`Self::create`] and append more
    /// records after whatever it already holds -- see this module's top doc
    /// comment for why the format supports this with no rewrite. Refuses a
    /// dump whose header does not match this build's current encoder
    /// widths, the same guard [`super::rankdata::read_shard`] applies to its
    /// own format.
    pub fn append(path: &Path) -> Result<Self, String> {
        let existing = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut r = Reader::new(&existing);
        let magic = r.take(MAGIC.len())?;
        if magic != MAGIC {
            return Err(format!("{}: not a policy dump (bad magic {magic:?})", path.display()));
        }
        let version = r.u32()?;
        if version != VERSION {
            return Err(format!(
                "{}: dump version {version}, this build reads version {VERSION} -- regenerate it",
                path.display()
            ));
        }
        let state_dim = r.u32()?;
        if state_dim as usize != ENCODING_DIM {
            return Err(header_mismatch(path, "state_dim", state_dim, ENCODING_DIM));
        }
        let action_dim = r.u32()?;
        if action_dim as usize != ACTION_DIM {
            return Err(header_mismatch(path, "action_dim", action_dim, ACTION_DIM));
        }
        let file = OpenOptions::new()
            .append(true)
            .open(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(DumpWriter { file })
    }

    /// Open `path` for appending, creating it (with a fresh header) if it
    /// does not exist yet -- the common case for a dumper CLI flag that
    /// names an output file once, run after run.
    pub fn create_or_append(path: &Path) -> Result<Self, String> {
        if path.exists() {
            Self::append(path)
        } else {
            Self::create(path)
        }
    }

    /// Append one record. Debug-asserts the state/action widths rather than
    /// silently writing a dump [`Self::append`] would later refuse to read
    /// back correctly -- a caller bug here should fail loudly at the write
    /// site, not resurface as a confusing read-time length error.
    pub fn write_record(&mut self, rec: &DecisionRecord) -> Result<(), String> {
        debug_assert_eq!(rec.state.len(), ENCODING_DIM, "record state width");
        for (i, a) in rec.legal.iter().enumerate() {
            debug_assert_eq!(a.len(), ACTION_DIM, "record legal[{i}] width");
        }
        debug_assert!(
            (rec.chosen as usize) < rec.legal.len(),
            "chosen index {} is out of range for {} legal actions",
            rec.chosen,
            rec.legal.len()
        );

        let mut buf = Vec::new();
        buf.push(rec.players);
        buf.push(rec.actor);
        push_u32(&mut buf, rec.legal.len() as u32);
        push_f64_slice(&mut buf, &rec.state);
        for a in &rec.legal {
            push_f64_slice(&mut buf, a);
        }
        push_u32(&mut buf, rec.chosen);
        push_f64_slice(&mut buf, &[rec.result]);
        self.file.write_all(&buf).map_err(|e| format!("write_record: {e}"))
    }
}

/// Read every record out of a dump written by [`DumpWriter`]. Loads the
/// whole file into memory (matching `net::load_checkpoint`'s precedent) --
/// fine for the modest row counts this format targets; a streaming reader
/// is a self-contained follow-up if a dump ever outgrows that.
pub fn read_dump(path: &Path) -> Result<Vec<DecisionRecord>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut r = Reader::new(&bytes);
    let magic = r.take(MAGIC.len())?;
    if magic != MAGIC {
        return Err(format!("{}: not a policy dump (bad magic {magic:?})", path.display()));
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(format!(
            "{}: dump version {version}, this build reads version {VERSION} -- regenerate it",
            path.display()
        ));
    }
    let state_dim = r.u32()? as usize;
    if state_dim != ENCODING_DIM {
        return Err(header_mismatch(path, "state_dim", state_dim as u32, ENCODING_DIM));
    }
    let action_dim = r.u32()? as usize;
    if action_dim != ACTION_DIM {
        return Err(header_mismatch(path, "action_dim", action_dim as u32, ACTION_DIM));
    }

    let mut out = Vec::new();
    while r.remaining() > 0 {
        let players = r.take(1)?[0];
        let actor = r.take(1)?[0];
        let legal_count = r.u32()? as usize;
        let state = r.f64_vec(state_dim)?;
        let mut legal = Vec::with_capacity(legal_count);
        for _ in 0..legal_count {
            legal.push(r.f64_vec(action_dim)?);
        }
        let chosen = r.u32()?;
        let result = r.f64()?;
        out.push(DecisionRecord { players, actor, state, legal, chosen, result });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(seed: u8) -> DecisionRecord {
        DecisionRecord {
            players: 3,
            actor: seed % 3,
            state: (0..ENCODING_DIM).map(|i| (i as f64 + f64::from(seed)) * 0.5).collect(),
            legal: (0..4)
                .map(|k| (0..ACTION_DIM).map(|i| (i as f64) - f64::from(k) - f64::from(seed)).collect())
                .collect(),
            chosen: 2,
            result: if seed % 2 == 0 { 1.0 } else { 0.0 },
        }
    }

    /// Writing then reading back a dump reproduces every field exactly --
    /// the round trip [`DumpWriter`]/[`read_dump`] exist to guarantee.
    #[test]
    fn dump_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("ttapolicy_dump_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("round_trip.tpd");

        let records = vec![sample_record(0), sample_record(1), sample_record(2)];
        let mut w = DumpWriter::create(&path).unwrap();
        for r in &records {
            w.write_record(r).unwrap();
        }
        drop(w);

        let back = read_dump(&path).unwrap();
        assert_eq!(back, records);
        std::fs::remove_file(&path).ok();
    }

    /// [`DumpWriter::append`] (via [`DumpWriter::create_or_append`]) adds
    /// records after an existing, already-headered file rather than
    /// clobbering it -- the property that makes this format "appendable"
    /// rather than "one shot per file".
    #[test]
    fn create_or_append_extends_an_existing_dump_without_clobbering_it() {
        let dir = std::env::temp_dir().join(format!("ttapolicy_dump_test_append_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("append.tpd");
        std::fs::remove_file(&path).ok();

        let first = sample_record(5);
        let second = sample_record(6);
        {
            let mut w = DumpWriter::create_or_append(&path).unwrap();
            w.write_record(&first).unwrap();
        }
        {
            let mut w = DumpWriter::create_or_append(&path).unwrap();
            w.write_record(&second).unwrap();
        }

        let back = read_dump(&path).unwrap();
        assert_eq!(back, vec![first, second]);
        std::fs::remove_file(&path).ok();
    }

    /// A dump whose header claims a different action width than this
    /// build's encoder actually produces is refused outright, matching
    /// `rankdata::read_shard`'s guard against training on a stale shard.
    #[test]
    fn read_dump_rejects_a_header_with_the_wrong_action_width() {
        let dir = std::env::temp_dir().join(format!("ttapolicy_dump_test_badwidth_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad_width.tpd");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        push_u32(&mut bytes, VERSION);
        push_u32(&mut bytes, ENCODING_DIM as u32);
        push_u32(&mut bytes, (ACTION_DIM + 1) as u32); // wrong on purpose
        std::fs::write(&path, &bytes).unwrap();

        let err = read_dump(&path).unwrap_err();
        assert!(err.contains("action_dim"), "error should name the mismatched field: {err}");
        std::fs::remove_file(&path).ok();
    }
}
