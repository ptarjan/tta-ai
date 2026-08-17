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
//! ([`super::net::Reader`]/`push_u32`/`push_f32_slice`, reused below rather
//! than re-invented a third time).
//!
//! ## Format version 2: the storage-cost fix
//!
//! Version 1 (slice 1's proof dump) stored `f64` throughout AND stored every
//! legal action as its full [`super::action::ACTION_DIM`]-wide DENSE feature
//! vector -- 1,159 decision records measured at 34.8 MB, ~30 KB/record,
//! which extrapolates to tens of GB for the hundreds-of-thousands-of-
//! decisions corpus a policy head actually needs to train on. Two
//! independent fixes, both in this version bump:
//!
//! 1. `f32`, not `f64`, for the state block -- nothing here needs more than
//!    single precision (matching `rankdata::Shard`'s own `f32` choice, whose
//!    doc comment makes the same argument for the value net's shards).
//! 2. Each legal action is stored as the COMPACT [`Move`] it already is --
//!    a handful of bytes (`write_move`/`read_move` below) -- rather than its
//!    dense one-hot-heavy [`super::action::ACTION_DIM`]-float encoding. A
//!    `Move` is overwhelmingly one-hot-shaped once expanded (see
//!    `action.rs`'s own doc comment: type one-hot + card one-hots + target
//!    one-hot + ...), so storing the compact enum and expanding it back to
//!    the dense vector AT TRAIN TIME, via [`super::action::encode_action`]
//!    (the same function that produced the dense vectors in the first
//!    place), keeps exactly ONE definition of the encoding. A reader that
//!    also stored its own copy of the expansion logic could silently drift
//!    from `encode_action` the moment either one changed.
//!
//! `write_move`/`read_move` build their four auxiliary bytes from
//! [`super::action::decode_aux`] -- the SAME extraction `encode_action`
//! itself calls -- rather than a second hand-written `match` on [`Move`]
//! that could disagree with it about which field a given variant carries.
//! The one-byte tag is [`super::action::move_kind_slot`], for the identical
//! reason.
//!
//! Also new in version 2: a `game_id` per record (see below) -- version 1
//! had no way to tell which records came from the same game, which a
//! trainer needs to split train/held-out BY GAME rather than by decision
//! (decisions inside one game are heavily correlated; a decision-level
//! split leaks). `bin/dump_selfplay.rs` sets `game_id` to
//! `players * 10_000_000 + seed`, which cannot collide across the 2p/3p/4p
//! runs a corpus is assembled from (see that binary's own doc comment) as
//! long as no single run's `--seed` reaches eight digits.
//!
//! ## On-disk format
//!
//! Little-endian throughout.
//!
//! **Header** (written once, at the start of the file):
//! ```text
//! magic:       8 bytes, b"TTAPDMP2"
//! version:     u32
//! state_dim:   u32   -- width of every `state` block below
//! action_dim:  u32   -- width `action::encode_action` expands one compact
//!                        `Move` to; a consistency check only (nothing
//!                        below is stored at this width any more)
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
//! game_id:     u32   -- which game this decision came from (see above)
//! legal_count: u32
//! state:       state_dim  f32s  -- encode::encode(state, actor), narrowed
//! legal[0]:    7 bytes -- write_move(legal_moves[0]) (compact `Move`)
//! ...
//! legal[legal_count-1]: 7 bytes
//! chosen:      u32   -- index into legal[] of the move actually played
//! result:      f32   -- actor's final win share (game::winners: 1.0 clean
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

use crate::cards::CardId;
use crate::moves::Move;

use super::action::{
    churchill_from_slot, churchill_slot, decode_aux, move_kind_slot, pact_side_from_slot, pact_side_slot, ACTION_DIM,
};
use super::encode::ENCODING_DIM;
use super::net::{push_f32_slice, push_u32, Reader};

const MAGIC: &[u8; 8] = b"TTAPDMP2";
const VERSION: u32 = 2;

/// Bytes one compact [`Move`] occupies on disk: `tag`(1) + `card_a`(2) +
/// `card_b`(2) + `aux1`(1) + `aux2`(1). Fixed-width (not variable-length),
/// so a reader that only wants a census can skip `legal_count` of these
/// without decoding any of them -- see this module's top doc comment.
const MOVE_BYTES: usize = 7;

/// One decision point: the public state, every legal action, which one was
/// chosen, and the actor's eventual result. Plain data, not four parallel
/// `Vec`s kept in step by index -- see `rankdata.rs::Shard`'s own doc
/// comment for why this project avoids that shape.
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionRecord {
    pub players: u8,
    pub actor: u8,
    /// Which game this decision came from -- see this module's top doc
    /// comment on why version 2 added this (the train/held-out split needs
    /// to group by game, not by decision).
    pub game_id: u32,
    /// `encode::encode(state, actor)`'s output, narrowed to `f32` -- length
    /// [`ENCODING_DIM`].
    pub state: Vec<f32>,
    /// The legal moves themselves, in `legal_moves`'s own order -- NOT their
    /// dense [`super::action::encode_action`] expansion; a trainer calls
    /// that function itself, once per move, at train time. See this
    /// module's top doc comment for why storing the compact `Move` here
    /// (rather than its expansion) is the whole storage-cost fix.
    pub legal: Vec<Move>,
    /// Index into `legal` of the move actually played.
    pub chosen: u32,
    /// The actor's final win share, backfilled once the game ends.
    pub result: f32,
}

fn header_mismatch(path: &Path, what: &str, got: u32, want: usize) -> String {
    format!(
        "{}: {what} {got} does not match this build's {what} {want} -- \
         regenerate this dump against the current encoders, do not read it as-is",
        path.display()
    )
}

// ============================================================ compact Move

/// Serialise `mv` as [`MOVE_BYTES`] bytes: a one-byte kind tag
/// ([`move_kind_slot`]) plus the primary/secondary card (`mv.card()`/
/// [`decode_aux`]'s `secondary`, `card_a`/`card_b`, [`CardId::NONE`] as the
/// "absent" sentinel) plus two auxiliary bytes wide enough for every other
/// field a `Move` variant can carry ([`decode_aux`]'s `target`/`slot`/
/// `steps`/`bid`/`choose` all fold onto `aux1` -- [`decode_aux`]'s own
/// `match` never sets two of them for the same variant -- and `side`/
/// `churchill` fold onto `aux2` for the same reason).
fn write_move(out: &mut Vec<u8>, mv: Move) {
    let tag = move_kind_slot(&mv) as u8;
    let card_a = mv.card().unwrap_or(CardId::NONE).0;
    let aux = decode_aux(mv);
    let card_b = aux.secondary.0;
    let aux1 = aux.target.or(aux.slot).or(aux.steps).or(aux.bid).or(aux.choose).unwrap_or(0);
    let aux2 = match (aux.side, aux.churchill) {
        (Some(s), None) => pact_side_slot(s) as u8,
        (None, Some(c)) => churchill_slot(c) as u8,
        (None, None) => 0,
        (Some(_), Some(_)) => unreachable!("decode_aux never sets both side and churchill for one Move"),
    };
    out.push(tag);
    out.extend_from_slice(&card_a.to_le_bytes());
    out.extend_from_slice(&card_b.to_le_bytes());
    out.push(aux1);
    out.push(aux2);
}

/// Inverse of [`write_move`]. The tag `match` below is deliberately NOT
/// exhaustive over every `u8` (it cannot be -- 219 of 256 tag values are not
/// a real move kind at all) but IS exhaustive over `0..NUM_MOVE_KINDS`
/// (checked by `compact_move_round_trips_for_every_variant` below against
/// one sample per [`Move`] variant); an out-of-range tag is corrupt-file
/// data, handled the same way this module's header checks handle a
/// mismatched dimension: a named `Err`, never a silent guess.
fn read_move(r: &mut Reader) -> Result<Move, String> {
    let tag = r.u8()?;
    let card_a = CardId(r.u16()?);
    let card_b = CardId(r.u16()?);
    let aux1 = r.u8()?;
    let aux2 = r.u8()?;
    let side = || pact_side_from_slot(aux2);
    let churchill = || churchill_from_slot(aux2);
    match tag {
        0 => Ok(Move::Take { slot: aux1, cost: i32::MAX }),
        1 => Ok(Move::Build { card: card_a }),
        2 => Ok(Move::Develop { card: card_a }),
        3 => Ok(Move::Upgrade { from: card_b, to: card_a }),
        4 => Ok(Move::WonderStep { steps: aux1 }),
        5 => Ok(Move::Pop),
        6 => Ok(Move::PopFree),
        7 => Ok(Move::Revolution { card: card_a }),
        8 => Ok(Move::PlayLeader { card: card_a }),
        9 => Ok(Move::PlayAction { card: card_a }),
        10 => Ok(Move::Destroy { card: card_a }),
        11 => Ok(Move::PlayTactic { card: card_a }),
        12 => Ok(Move::CopyTactic { card: card_a }),
        13 => Ok(Move::Aggression { card: card_a, target: aux1 }),
        14 => Ok(Move::War { card: card_a, target: aux1 }),
        15 => Ok(Move::OfferPact { card: card_a, target: aux1, side: side()? }),
        16 => Ok(Move::CancelPact { owner: aux1 }),
        17 => Ok(Move::PrepareEvent { card: card_a }),
        18 => Ok(Move::RemoveLeaderYellow),
        19 => Ok(Move::ColumbusColonize { card: card_a }),
        20 => Ok(Move::Barbarossa { card: card_a }),
        21 => Ok(Move::BachTheater { from: card_b, to: card_a }),
        22 => Ok(Move::TradeFoodAsResource),
        23 => Ok(Move::TradeResourceAsFood),
        24 => Ok(Move::Bid { n: aux1 }),
        25 => Ok(Move::BidPass),
        26 => Ok(Move::Defend { card: card_a }),
        27 => Ok(Move::DefendDone),
        28 => Ok(Move::SendUnit { card: card_a }),
        29 => Ok(Move::SendBonus { card: card_a }),
        30 => Ok(Move::SendDiscard { card: card_a }),
        31 => Ok(Move::SendDone),
        32 => Ok(Move::Choose { n: aux1 }),
        33 => Ok(Move::Churchill { choice: churchill()? }),
        34 => Ok(Move::EndTurn),
        35 => Ok(Move::PolPass),
        36 => Ok(Move::Resign),
        other => Err(format!("read_move: tag {other} out of range 0..37")),
    }
}

// ================================================================== writer

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

    /// Append one record. Debug-asserts the state width rather than
    /// silently writing a dump [`Self::append`] would later refuse to read
    /// back correctly -- a caller bug here should fail loudly at the write
    /// site, not resurface as a confusing read-time length error.
    pub fn write_record(&mut self, rec: &DecisionRecord) -> Result<(), String> {
        debug_assert_eq!(rec.state.len(), ENCODING_DIM, "record state width");
        debug_assert!(
            (rec.chosen as usize) < rec.legal.len(),
            "chosen index {} is out of range for {} legal actions",
            rec.chosen,
            rec.legal.len()
        );

        let mut buf = Vec::new();
        buf.push(rec.players);
        buf.push(rec.actor);
        push_u32(&mut buf, rec.game_id);
        push_u32(&mut buf, rec.legal.len() as u32);
        push_f32_slice(&mut buf, &rec.state);
        for &mv in &rec.legal {
            write_move(&mut buf, mv);
        }
        push_u32(&mut buf, rec.chosen);
        push_f32_slice(&mut buf, &[rec.result]);
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
        let game_id = r.u32()?;
        let legal_count = r.u32()? as usize;
        let state = r.f32_vec(state_dim)?;
        let mut legal = Vec::with_capacity(legal_count);
        for _ in 0..legal_count {
            legal.push(read_move(&mut r)?);
        }
        let chosen = r.u32()?;
        let result = r.f32()?;
        out.push(DecisionRecord { players, actor, game_id, state, legal, chosen, result });
    }
    Ok(out)
}

/// Bytes one record with `legal_count` legal actions and [`ENCODING_DIM`]
/// state width occupies on disk -- exposed so `bin/dump_selfplay.rs` can
/// report the measured shrink factor against version 1 without re-deriving
/// this arithmetic.
pub fn record_bytes(legal_count: usize) -> usize {
    1 + 1 + 4 + 4 + ENCODING_DIM * 4 + legal_count * MOVE_BYTES + 4 + 4
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moves::{ChurchillChoice, PactSide};

    fn sample_record(seed: u8) -> DecisionRecord {
        DecisionRecord {
            players: 3,
            actor: seed % 3,
            game_id: seed as u32,
            state: (0..ENCODING_DIM).map(|i| (i as f32 + f32::from(seed)) * 0.5).collect(),
            legal: vec![
                Move::Take { slot: seed % 4, cost: i32::MAX },
                Move::Build { card: CardId(seed as u16) },
                Move::EndTurn,
                Move::Choose { n: 2 },
            ],
            chosen: 2,
            result: if seed.is_multiple_of(2) { 1.0 } else { 0.0 },
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

    /// The property the storage-cost fix depends on: compact-encoding then
    /// decoding a `Move`, then running it through
    /// [`super::super::action::encode_action`] -- the SAME expansion a
    /// trainer would call on a value read back from disk -- produces the
    /// bit-identical dense vector encoding the original `Move` directly
    /// would have. One sample per variant, matching `action.rs`'s own
    /// `move_kind_slot_is_injective_and_in_range` sample list, so a variant
    /// this codec forgot how to round-trip is caught here rather than
    /// silently mis-decoding at train time.
    #[test]
    fn compact_move_round_trips_for_every_variant() {
        use super::super::action::encode_action;
        let card = CardId::by_name("Warriors").unwrap();
        let card2 = CardId::by_name("Irrigation").unwrap();
        let samples = [
            Move::Take { slot: 3, cost: i32::MAX },
            Move::Build { card },
            Move::Develop { card },
            Move::Upgrade { from: card2, to: card },
            Move::WonderStep { steps: 2 },
            Move::Pop,
            Move::PopFree,
            Move::Revolution { card },
            Move::PlayLeader { card },
            Move::PlayAction { card },
            Move::Destroy { card },
            Move::PlayTactic { card },
            Move::CopyTactic { card },
            Move::Aggression { card, target: 2 },
            Move::War { card, target: 1 },
            Move::OfferPact { card, target: 3, side: PactSide::B },
            Move::CancelPact { owner: 1 },
            Move::PrepareEvent { card },
            Move::RemoveLeaderYellow,
            Move::ColumbusColonize { card },
            Move::Barbarossa { card },
            Move::BachTheater { from: card2, to: card },
            Move::TradeFoodAsResource,
            Move::TradeResourceAsFood,
            Move::Bid { n: 5 },
            Move::BidPass,
            Move::Defend { card },
            Move::DefendDone,
            Move::SendUnit { card },
            Move::SendBonus { card },
            Move::SendDiscard { card },
            Move::SendDone,
            Move::Choose { n: 7 },
            Move::Churchill { choice: ChurchillChoice::Military },
            Move::EndTurn,
            Move::PolPass,
            Move::Resign,
        ];
        assert_eq!(samples.len(), super::super::action::NUM_MOVE_KINDS, "one sample per variant, or this test is stale");
        for mv in samples {
            let mut buf = Vec::new();
            write_move(&mut buf, mv);
            assert_eq!(buf.len(), MOVE_BYTES);
            let mut r = Reader::new(&buf);
            let back = read_move(&mut r).unwrap();
            assert_eq!(back, mv, "compact round trip changed the move");
            assert_eq!(
                encode_action(0, mv),
                encode_action(0, back),
                "{mv:?}: dense expansion differs after a compact round trip"
            );
        }
    }

    /// A tag byte outside `0..NUM_MOVE_KINDS` is corrupt-file data and must
    /// fail loudly, not be misread as some arbitrary variant.
    #[test]
    fn read_move_rejects_an_out_of_range_tag() {
        let mut buf = vec![37u8]; // one past the last valid tag (Resign = 36)
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.push(0);
        buf.push(0);
        let mut r = Reader::new(&buf);
        let err = read_move(&mut r).unwrap_err();
        assert!(err.contains("37"), "{err}");
    }

    /// Version 2's whole point: a record's on-disk size no longer scales
    /// with `legal_count * ACTION_DIM` (version 1's dense-vector cost), only
    /// `legal_count * MOVE_BYTES` -- pins the arithmetic
    /// [`record_bytes`]/`bin/dump_selfplay.rs`'s reported shrink factor
    /// depend on, rather than trusting it by inspection alone.
    #[test]
    fn record_bytes_scales_with_move_bytes_not_action_dim() {
        let with_5 = record_bytes(5);
        let with_6 = record_bytes(6);
        assert_eq!(with_6 - with_5, MOVE_BYTES);
        const { assert!(MOVE_BYTES < ACTION_DIM * 4, "compact move must be far smaller than one dense f32 action vector") };
    }
}
