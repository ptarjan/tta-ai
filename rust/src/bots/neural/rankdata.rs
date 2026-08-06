//! Training-data generation for the value net: ranking pairs and value
//! anchors, drawn from Rust self-play. Ports the SUBSTANCE of
//! `experiments/neural_rankdata.py` (167 lines) -- read that file's own top
//! doc comment first for the objective this feeds (a learning-to-rank loss
//! over sibling moves, because squared-error regression spends ~no capacity
//! on the difference between one action apart states -- `docs/BOT_ARCHITECTURE.md`
//! 3b, `docs/NEURAL_EVAL.md`).
//!
//! Per sampled decision, from the mover's perspective, this records:
//!   * a **value anchor** -- the pre-move state encoding, labelled with the
//!     mover's eventual final-culture margin; and
//!   * up to `krej` **ranking pairs** -- the encoding of the child state the
//!     TEACHER chose, against the encodings of children it rejected.
//!
//! ## The bug this port does NOT reproduce: hidden information in the labels
//!
//! `neural_rankdata.py::_child_enc` applies each candidate move to the REAL
//! state, with no determinization:
//!
//! ```python
//! def _child_enc(state, mv, seat):
//!     t = copy_state(state)
//!     actions.apply(t, mv, _TRIAL_RNG)
//!     return E.encode(t, seat)
//! ```
//!
//! `docs/NEURAL_LOOP_NULL.md` 3.4 measured the consequence: ~95% of
//! `end_turn` candidates draw a card, and because the root was never
//! determinized, an encoded `end_turn` child carries the TRUE next civil
//! card -- something the mover cannot see when the search actually plays,
//! since `NeuralBot`/`NeuralPlanBot` always determinize at the root. That is
//! exactly the class of bug this project's standing rule forbids: card
//! counting and public information are fair game, but the undealt deck
//! order is not something any player at the table can know, and training on
//! it teaches the net to price a move off information it will never have at
//! play time.
//!
//! `experiments/plan_teacher_gen.py` already carries the fix (its own top
//! doc comment calls this out as bug #2 of two it closes), and this module
//! carries the same fix forward: every candidate child is applied to a
//! COPY of one shared [`determinize`]d root per sampled decision (`bots::
//! plan::determinize`, which reshuffles exactly the three piles a player
//! cannot see -- the two draw decks and the future-events pile), so the
//! "chosen" and "rejected" encodings are compared on the same honest
//! hypothetical rather than one of them leaking the truth.
//!
//! The value anchor itself needs no such fix: it encodes the literal current
//! state from the mover's own perspective with no hypothetical move applied,
//! which is exactly what the mover legitimately knows.
//!
//! ## What this port intentionally leaves out
//!
//! `plan_teacher_gen.py`'s OTHER fix -- sampling value rows from the leaf
//! positions a whole-turn beam search actually scores, rather than pre-move
//! states -- needs a hook into the beam's internal scoring loop
//! (`TeacherPlanBot._score`/`NeuralPlanBot._score_many`). That is a
//! substantially bigger change to `bots::plan`'s search internals than this
//! module's job of reproducing `neural_rankdata.py`, so it is left for
//! whoever next needs a turn-boundary-only value distribution, not silently
//! folded in here.
//!
//! ## The vacuity hazard (`docs/NEURAL_LOOP_NULL.md`)
//!
//! The 41-hour null happened because ranking pairs were labelled with the
//! very model being warm-started from: the untrained net already satisfied
//! 97.6% of its own "chosen vs rejected" pairs, so gradient descent had
//! nothing to teach. This generator's teacher is always a classical bot
//! (`bots::greedy::BotKind`), never the neural net being trained, so it
//! cannot fall into that specific trap by construction -- but a downstream
//! consumer has no way to know that unless the data says so. So every
//! [`Shard`] carries a `teacher` tag (bot spec, plus weights path if any)
//! recorded once per shard -- cheap, since one generation run uses exactly
//! one teacher for every row in it. If this generator ever grows a
//! checkpoint-driven teacher (mirroring `experiments/neural_gen_plan.py`),
//! that tag is what lets a trainer compare "who chose these labels" against
//! "what am I warm-starting from" and refuse to train on a fixed point.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::apply;
use crate::bots::greedy::{self, Seat};
use crate::bots::plan::determinize;
use crate::game;
use crate::legal;
use crate::moves::Move;
use crate::rng::PyRandom;
use crate::state::GameState;

use super::encode::{self, ENCODING_DIM};

// =============================================================== in-memory

/// One ranking pair: the encoding of the child the teacher chose, and the
/// encoding of a child it rejected at the same decision. `f32`, not the
/// `f64` [`encode::encode`] returns -- half the footprint of a value never
/// asked to round-trip more precision than that (Python stores these as
/// `float16`; see [`write_shard`]'s doc comment for why this port picked
/// `f32` instead).
#[derive(Clone, Debug, PartialEq)]
pub struct RankPair {
    pub chosen: Vec<f32>,
    pub rejected: Vec<f32>,
}

/// One value anchor: a pre-move state encoding and the mover's eventual
/// final-culture margin.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueRow {
    pub state: Vec<f32>,
    pub margin: f32,
}

/// A batch of training rows plus the header fields [`write_shard`]/
/// [`read_shard`] round-trip. One collection of [`RankPair`]/[`ValueRow`]
/// structs, not four parallel arrays (`Xa`/`Xb`/`Xv`/`yv`, Python's shape):
/// keeping a chosen encoding and its rejected sibling in step by a shared
/// row index is exactly the kind of parallel-collection bug this project
/// keeps finding elsewhere.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shard {
    /// Which bot produced every label in this shard (spec string, plus the
    /// weights path if one was given). See this module's top doc comment,
    /// "The vacuity hazard".
    pub teacher: String,
    pub pairs: Vec<RankPair>,
    pub values: Vec<ValueRow>,
}

impl Shard {
    pub fn new(teacher: String) -> Shard {
        Shard { teacher, pairs: Vec::new(), values: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty() && self.values.is_empty()
    }
}

// ================================================================= on disk

/// Bumped whenever the byte layout below changes incompatibly. [`read_shard`]
/// refuses anything else outright rather than guessing.
pub const SHARD_VERSION: u32 = 1;

const MAGIC: &[u8; 4] = b"RKD1";

/// `f32`, not Python's `float16`. Two places already have to agree on the
/// encoding WIDTH for this format to be safe at all (see [`read_shard`]);
/// adding a hand-rolled half-precision codec as a THIRD thing to keep in
/// step, in a crate whose `[dependencies]` is empty on principle, is not a
/// trade worth making for a format this project has already been burned by
/// twice. If shard size becomes the bottleneck, packing to `f16` is a
/// self-contained follow-up against this same [`Shard`] type -- it does not
/// need to happen here.
fn write_u32(w: &mut impl Write, v: u32) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_u64(w: &mut impl Write, v: u64) -> io::Result<()> {
    w.write_all(&v.to_le_bytes())
}

fn write_f32_slice(w: &mut impl Write, v: &[f32]) -> io::Result<()> {
    // One `write_all` over a byte buffer beats one syscall-ish call per
    // float; encoding rows are 1906 floats each and this runs once per row.
    let mut buf = Vec::with_capacity(v.len() * 4);
    for &x in v {
        buf.extend_from_slice(&x.to_le_bytes());
    }
    w.write_all(&buf)
}

fn read_u32(r: &mut impl Read) -> io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn read_f32_vec(r: &mut impl Read, n: usize) -> io::Result<Vec<f32>> {
    let mut buf = vec![0u8; n * 4];
    r.read_exact(&mut buf)?;
    Ok(buf.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

fn ctx(path: &Path, e: io::Error) -> String {
    format!("{}: {e}", path.display())
}

/// Write `shard` to `path` in this module's native binary format.
///
/// Layout (all integers little-endian): 4-byte magic `b"RKD1"`, `u32`
/// version, `u32` encoding width, `u32` teacher-tag length followed by that
/// many UTF-8 bytes, `u64` pair count, `u64` value-row count, then every
/// pair (`chosen` then `rejected`, `width` `f32`s each) followed by every
/// value row (`state`, `width` `f32`s, then `margin`, one `f32`).
///
/// The width written is always [`ENCODING_DIM`] -- this function does not
/// accept a caller-supplied width, so there is no way to write a
/// self-contradictory shard by mistake. (The round-trip test that writes an
/// intentionally-wrong width to exercise [`read_shard`]'s guard does so by
/// hand-assembling bytes, not through this function -- see this module's
/// `#[cfg(test)]` block.)
pub fn write_shard(path: &Path, shard: &Shard) -> Result<(), String> {
    let f = File::create(path).map_err(|e| ctx(path, e))?;
    let mut w = BufWriter::new(f);
    (|| -> io::Result<()> {
        w.write_all(MAGIC)?;
        write_u32(&mut w, SHARD_VERSION)?;
        write_u32(&mut w, ENCODING_DIM as u32)?;
        let tag = shard.teacher.as_bytes();
        write_u32(&mut w, tag.len() as u32)?;
        w.write_all(tag)?;
        write_u64(&mut w, shard.pairs.len() as u64)?;
        write_u64(&mut w, shard.values.len() as u64)?;
        for p in &shard.pairs {
            debug_assert_eq!(p.chosen.len(), ENCODING_DIM, "pair row width");
            debug_assert_eq!(p.rejected.len(), ENCODING_DIM, "pair row width");
            write_f32_slice(&mut w, &p.chosen)?;
            write_f32_slice(&mut w, &p.rejected)?;
        }
        for v in &shard.values {
            debug_assert_eq!(v.state.len(), ENCODING_DIM, "value row width");
            write_f32_slice(&mut w, &v.state)?;
            write_f32_slice(&mut w, &[v.margin])?;
        }
        w.flush()
    })()
    .map_err(|e| ctx(path, e))
}

/// Read a shard written by [`write_shard`].
///
/// Refuses, with a message naming the exact mismatch, rather than silently
/// misreading:
///   * a bad magic (not a shard this module wrote at all);
///   * a version other than [`SHARD_VERSION`];
///   * an encoding width that does not match [`ENCODING_DIM`] **as compiled
///     into this binary right now**. This is the guard the encoder-width
///     bug (1907 -> 1906) is here to prevent from happening silently again:
///     a shard generated against a stale encoder is refused outright, never
///     zero-padded or truncated to fit.
pub fn read_shard(path: &Path) -> Result<Shard, String> {
    let f = File::open(path).map_err(|e| ctx(path, e))?;
    let mut r = BufReader::new(f);

    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| ctx(path, e))?;
    if &magic != MAGIC {
        return Err(format!(
            "{}: not a rankdata shard (bad magic {magic:?}, want {MAGIC:?})",
            path.display()
        ));
    }
    let version = read_u32(&mut r).map_err(|e| ctx(path, e))?;
    if version != SHARD_VERSION {
        return Err(format!(
            "{}: shard version {version}, this build reads version {SHARD_VERSION} -- regenerate it",
            path.display()
        ));
    }
    let dim = read_u32(&mut r).map_err(|e| ctx(path, e))? as usize;
    if dim != ENCODING_DIM {
        return Err(format!(
            "{}: shard encoding width {dim} does not match this build's encoder width {ENCODING_DIM} -- \
             regenerate this shard against the current encoder, do not train on it as-is",
            path.display()
        ));
    }
    let tag_len = read_u32(&mut r).map_err(|e| ctx(path, e))? as usize;
    let mut tag_buf = vec![0u8; tag_len];
    r.read_exact(&mut tag_buf).map_err(|e| ctx(path, e))?;
    let teacher = String::from_utf8(tag_buf)
        .map_err(|e| format!("{}: teacher tag is not valid UTF-8: {e}", path.display()))?;

    let n_pairs = read_u64(&mut r).map_err(|e| ctx(path, e))? as usize;
    let n_values = read_u64(&mut r).map_err(|e| ctx(path, e))? as usize;

    let mut pairs = Vec::with_capacity(n_pairs);
    for _ in 0..n_pairs {
        let chosen = read_f32_vec(&mut r, dim).map_err(|e| ctx(path, e))?;
        let rejected = read_f32_vec(&mut r, dim).map_err(|e| ctx(path, e))?;
        pairs.push(RankPair { chosen, rejected });
    }
    let mut values = Vec::with_capacity(n_values);
    for _ in 0..n_values {
        let state = read_f32_vec(&mut r, dim).map_err(|e| ctx(path, e))?;
        let margin = read_f32_vec(&mut r, 1).map_err(|e| ctx(path, e))?[0];
        values.push(ValueRow { state, margin });
    }
    Ok(Shard { teacher, pairs, values })
}

// ============================================================== generation

fn to_f32(v: &[f64]) -> Vec<f32> {
    v.iter().map(|&x| x as f32).collect()
}

/// `neural_rankdata.py::_final_margins`: each player's final culture minus
/// the best of the others' (0 if nobody else is in the game). Split out from
/// [`final_margins`] so it is testable without a real [`GameState`].
fn margins_from_scores(sc: &[i32]) -> Vec<f64> {
    sc.iter()
        .enumerate()
        .map(|(i, &s)| {
            let best_other = sc
                .iter()
                .enumerate()
                .filter(|&(j, _)| j != i)
                .map(|(_, &v)| v)
                .max()
                .unwrap_or(0);
            f64::from(s - best_other)
        })
        .collect()
}

fn final_margins(state: &GameState) -> Vec<f64> {
    margins_from_scores(&game::scores(state))
}

/// Fisher-Yates over `v` using [`PyRandom::random`]. Not a port of CPython's
/// `random.shuffle` (that needs `_randbelow`, which [`PyRandom`] does not
/// expose beyond [`crate::rng::shuffle_cards`]'s `CardId`-specific use) --
/// this project's own standing rule is bit-identical output is not the bar,
/// correctness is, and picking `krej` rejected siblings uniformly at random
/// does not need to replay Python's exact draw sequence.
fn shuffle_indices(rng: &mut PyRandom, v: &mut [usize]) {
    for i in (1..v.len()).rev() {
        let j = ((rng.random() * (i + 1) as f64) as usize).min(i);
        v.swap(i, j);
    }
}

/// One sampled decision's worth of rows: everything [`play_and_record`]
/// needs to build its return value, `let`-bound in one struct instead of
/// two more parallel vectors of "pending" state alongside `pairs`.
struct PendingValue {
    state: Vec<f32>,
    seat: u8,
}

/// Play one game with `seats` as the teacher and record ranking pairs and
/// (seat-labelled, not-yet-margin-resolved) value rows. Mirrors
/// `neural_rankdata.py::play_and_record`; see this module's top doc comment
/// for the one behavioural fix (determinized children).
///
/// `seats` describes the teacher (kind + weights) round-robined over the
/// table by [`greedy::make_seats`]; fresh [`Bot`]s are built from it every
/// game, seeded from `seed`, exactly as Python constructs a fresh `BookBot`
/// list per game.
///
/// `pub`, not just `pub(crate)`, so a caller that wants to parallelize game
/// generation across threads -- `bin/rankdata.rs` does exactly this -- can
/// play each game OUTSIDE whatever lock guards its [`ShardWriter`], and only
/// take that lock for the cheap [`ShardWriter::push_game`] call. Bundling
/// simulation and writing into one locked call would force a caller to hold
/// the writer's lock for the length of a whole game to get both, which
/// serializes exactly the work threading exists to parallelize.
pub fn play_and_record(
    seats: &[Seat],
    players: u8,
    seed: u64,
    stride: usize,
    krej: usize,
    epsilon: f64,
) -> (Vec<RankPair>, Vec<ValueRow>) {
    let mut bots = greedy::build_bots(seats, seed as i64);
    let mut rng = PyRandom::new(seed.wrapping_mul(7919).wrapping_add(17) as i64);
    let mut state = game::new_game(players, seed);

    let mut pairs = Vec::new();
    let mut pending: Vec<PendingValue> = Vec::new();
    let mut ply = 0usize;
    let mut moves_played = 0usize;

    while !state.game_over && moves_played < game::MOVE_CAP {
        let legal = legal::legal_moves(&state);
        if legal.is_empty() {
            break;
        }
        let dec = state.decider();
        let non_resign: Vec<Move> =
            legal.as_slice().iter().copied().filter(|m| *m != Move::Resign).collect();
        let live: Vec<Move> = if non_resign.is_empty() { legal.as_slice().to_vec() } else { non_resign };

        let chosen = bots[dec as usize].pick(&state);

        // Only sample a decision whose chosen move is actually a member of
        // `live`: a teacher that resigned while non-resign options existed
        // has nothing to teach a sibling-preference objective (there would
        // be no honest "child of the chosen move" to rank against).
        // `neural_rankdata.py` handles this by re-`pick`ing over `live`
        // alone; `Bot::pick` has no such "pick from this subset" entry
        // point, so this port skips the sample instead of forcing one --
        // strictly more conservative, since it can only ever produce
        // *fewer* rows, never a mislabelled one.
        let sample = ply % stride == 0 && live.len() > 1 && live.contains(&chosen);

        if sample {
            pending.push(PendingValue { state: to_f32(&encode::encode(&state, dec)), seat: dec });

            // One shared determinization for every candidate at this
            // decision, so "chosen" and "rejected" are compared on the same
            // honest hypothetical world instead of one of them (usually
            // `end_turn`) leaking the true next card. See this module's top
            // doc comment.
            let mut dstate = state.clone();
            determinize(&mut dstate, &mut rng);
            let mut encs = Vec::with_capacity(live.len());
            for &mv in &live {
                let mut t = dstate.clone();
                apply::apply(&mut t, mv);
                encs.push(to_f32(&encode::encode(&t, dec)));
            }

            if let Some(ci) = live.iter().position(|&m| m == chosen) {
                let mut rej: Vec<usize> = (0..live.len()).filter(|&j| j != ci).collect();
                shuffle_indices(&mut rng, &mut rej);
                for &j in rej.iter().take(krej) {
                    pairs.push(RankPair { chosen: encs[ci].clone(), rejected: encs[j].clone() });
                }
            }
        }

        // Advance with a small epsilon of random exploration for state
        // diversity; the recorded label above is always the teacher's
        // preferred move at the state actually visited, exactly as Python's
        // comment on this line explains.
        let mv = if epsilon > 0.0 && rng.random() < epsilon {
            let i = ((rng.random() * live.len() as f64) as usize).min(live.len() - 1);
            live[i]
        } else {
            chosen
        };
        apply::apply(&mut state, mv);
        moves_played += 1;
        ply += 1;
    }

    let margins = final_margins(&state);
    let values = pending
        .into_iter()
        .map(|p| ValueRow { state: p.state, margin: margins[p.seat as usize] as f32 })
        .collect();
    (pairs, values)
}

// ========================================================== shard writer

/// Accumulates rows across games and flushes a numbered shard file whenever
/// either buffer reaches `shard_cap` -- mirroring `neural_rankdata.py`'s
/// `flush()`, which writes both buffers together whenever `pa` or `pv` hits
/// `args.shard`. Not thread-safe on its own; callers needing concurrent
/// generation share one behind a `Mutex` (see `bin/rankdata.rs`).
pub struct ShardWriter {
    out_prefix: PathBuf,
    teacher: String,
    shard_cap: usize,
    next_index: u32,
    buf: Shard,
    total_pairs: u64,
    total_values: u64,
}

/// What [`ShardWriter::flush`] reports: the file it wrote and how many rows
/// of each kind it holds.
pub struct FlushedShard {
    pub path: PathBuf,
    pub pairs: usize,
    pub values: usize,
}

impl ShardWriter {
    pub fn new(out_prefix: PathBuf, teacher: String, shard_cap: usize) -> ShardWriter {
        ShardWriter {
            out_prefix,
            teacher: teacher.clone(),
            shard_cap: shard_cap.max(1),
            next_index: 0,
            buf: Shard::new(teacher),
            total_pairs: 0,
            total_values: 0,
        }
    }

    pub fn total_pairs(&self) -> u64 {
        self.total_pairs
    }

    pub fn total_values(&self) -> u64 {
        self.total_values
    }

    /// Fold one game's rows in, flushing as many full shards as that
    /// produces (almost always zero or one; the `while` only matters if a
    /// single game somehow out-produces `shard_cap`, which `--shard`'s
    /// default of 120,000 makes essentially impossible).
    pub fn push_game(
        &mut self,
        pairs: Vec<RankPair>,
        values: Vec<ValueRow>,
    ) -> Result<Vec<FlushedShard>, String> {
        self.total_pairs += pairs.len() as u64;
        self.total_values += values.len() as u64;
        self.buf.pairs.extend(pairs);
        self.buf.values.extend(values);
        let mut flushed = Vec::new();
        while self.buf.pairs.len() >= self.shard_cap || self.buf.values.len() >= self.shard_cap {
            flushed.push(self.flush()?);
        }
        Ok(flushed)
    }

    fn flush(&mut self) -> Result<FlushedShard, String> {
        let path = self.out_prefix.with_file_name(format!(
            "{}.{:04}.rkd",
            self.out_prefix.file_name().and_then(|s| s.to_str()).unwrap_or("shard"),
            self.next_index
        ));
        let shard = std::mem::replace(&mut self.buf, Shard::new(self.teacher.clone()));
        let pairs = shard.pairs.len();
        let values = shard.values.len();
        write_shard(&path, &shard)?;
        self.next_index += 1;
        Ok(FlushedShard { path, pairs, values })
    }

    /// Write whatever remains, if anything. Callers must call this once
    /// after the last [`push_game`] or trailing rows are lost -- exactly
    /// the role Python's final unconditional `flush()` plays.
    pub fn finish(&mut self) -> Result<Option<FlushedShard>, String> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        Ok(Some(self.flush()?))
    }
}

// ===================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("tta_rankdata_test_{}_{tag}.rkd", std::process::id()))
    }

    fn dummy_row(fill: f32) -> Vec<f32> {
        vec![fill; ENCODING_DIM]
    }

    #[test]
    fn a_shard_round_trips_through_write_then_read_exactly() {
        let path = temp_path("roundtrip");
        let mut shard = Shard::new("weighted@champion_2p.json".to_string());
        shard.pairs.push(RankPair { chosen: dummy_row(1.5), rejected: dummy_row(-2.25) });
        shard.pairs.push(RankPair { chosen: dummy_row(0.0), rejected: dummy_row(3.0) });
        shard.values.push(ValueRow { state: dummy_row(7.0), margin: -41.5 });

        write_shard(&path, &shard).unwrap();
        let back = read_shard(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert_eq!(back, shard);
    }

    #[test]
    fn an_empty_shard_round_trips() {
        let path = temp_path("empty");
        let shard = Shard::new("plan".to_string());
        write_shard(&path, &shard).unwrap();
        let back = read_shard(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back, shard);
    }

    /// The bug this format exists to make impossible: the encoder's width
    /// changed once already (1907 -> 1906) with nothing checking that a
    /// shard's width and the encoder's width still agreed. Hand-assemble a
    /// shard whose header claims a width one wider than [`ENCODING_DIM`]
    /// and confirm [`read_shard`] refuses it by name rather than
    /// misinterpreting the byte stream.
    #[test]
    fn a_shard_written_at_one_feature_width_refuses_to_load_at_another() {
        let path = temp_path("wrong_width");
        let bad_dim = (ENCODING_DIM + 1) as u32;
        {
            let f = File::create(&path).unwrap();
            let mut w = BufWriter::new(f);
            w.write_all(MAGIC).unwrap();
            write_u32(&mut w, SHARD_VERSION).unwrap();
            write_u32(&mut w, bad_dim).unwrap();
            write_u32(&mut w, 0).unwrap(); // empty teacher tag
            write_u64(&mut w, 0).unwrap(); // no pairs
            write_u64(&mut w, 0).unwrap(); // no values
        }
        let err = read_shard(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(
            err.contains(&bad_dim.to_string()) && err.contains(&ENCODING_DIM.to_string()),
            "error should name both widths, got: {err}"
        );
    }

    #[test]
    fn a_shard_with_a_stale_version_header_is_rejected() {
        let path = temp_path("stale_version");
        {
            let f = File::create(&path).unwrap();
            let mut w = BufWriter::new(f);
            w.write_all(MAGIC).unwrap();
            write_u32(&mut w, SHARD_VERSION + 1).unwrap();
        }
        let err = read_shard(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.contains("version"), "error should mention version, got: {err}");
    }

    #[test]
    fn a_shard_with_the_wrong_magic_is_rejected_with_a_clear_message() {
        let path = temp_path("bad_magic");
        std::fs::write(&path, b"NOPE!!!!").unwrap();
        let err = read_shard(&path).unwrap_err();
        std::fs::remove_file(&path).ok();
        assert!(err.contains("magic"), "error should mention magic, got: {err}");
    }

    #[test]
    fn a_missing_shard_file_is_a_named_io_error_not_a_panic() {
        let path = temp_path("does_not_exist");
        let err = read_shard(&path).unwrap_err();
        assert!(err.contains(&path.display().to_string()));
    }

    #[test]
    fn final_margins_are_each_players_score_minus_the_best_of_the_others() {
        assert_eq!(margins_from_scores(&[100, 80, 60]), vec![20.0, -20.0, -40.0]);
        // A two-way tie for the lead: both leaders' margin is 0.
        assert_eq!(margins_from_scores(&[50, 50, 10]), vec![0.0, 0.0, -40.0]);
    }

    #[test]
    fn shuffle_indices_produces_a_permutation_of_the_input() {
        let mut rng = PyRandom::new(42);
        let mut v: Vec<usize> = (0..10).collect();
        shuffle_indices(&mut rng, &mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>(), "shuffle must not drop or duplicate indices");
    }

    #[test]
    fn playing_two_small_games_produces_value_rows_and_stays_at_the_current_encoder_width() {
        let seats =
            greedy::make_seats("weighted", 2, crate::bots::weighted::weights::Weights::default())
                .unwrap();
        let (pairs, values) = play_and_record(&seats, 2, 1, 3, 4, 0.05);
        assert!(!values.is_empty(), "a stride-3 sampled game should yield at least one value row");
        for v in &values {
            assert_eq!(v.state.len(), ENCODING_DIM);
        }
        for p in &pairs {
            assert_eq!(p.chosen.len(), ENCODING_DIM);
            assert_eq!(p.rejected.len(), ENCODING_DIM);
        }
    }

    #[test]
    fn shard_writer_flushes_exactly_when_a_buffer_reaches_its_cap() {
        let dir = std::env::temp_dir();
        let prefix = dir.join(format!("tta_rankdata_test_{}_writer_flush", std::process::id()));
        let mut w = ShardWriter::new(prefix.clone(), "weighted".to_string(), 3);
        let pairs = vec![
            RankPair { chosen: dummy_row(1.0), rejected: dummy_row(2.0) },
            RankPair { chosen: dummy_row(1.0), rejected: dummy_row(2.0) },
            RankPair { chosen: dummy_row(1.0), rejected: dummy_row(2.0) },
        ];
        let flushed = w.push_game(pairs, Vec::new()).unwrap();
        assert_eq!(flushed.len(), 1, "3 pairs at cap 3 should flush exactly one shard");
        assert_eq!(flushed[0].pairs, 3);
        assert!(w.finish().unwrap().is_none(), "nothing left to flush after an exact-cap flush");
        std::fs::remove_file(&flushed[0].path).ok();
    }
}
