//! Raw C-ABI wrapper around `tta::advisor` for `wasm32-unknown-unknown` --
//! the whole point being a browser can run the advisor with NO backend
//! server. See `/private/tmp/wasmify_notes.txt` for the exact ABI, an
//! example request/response, and what does not work in wasm.
//!
//! Zero dependencies (`Cargo.toml`'s own comment), matching the parent
//! crate's rule: no `wasm-bindgen`, no `serde`. Three exports carry the whole
//! interface -- `alloc`/`dealloc` (shared-memory buffer management) and
//! `advise` (the one real entry point) -- plus a fourth, `last_error`, purely
//! as a post-mortem aid for the panic-abort case (see that function's own
//! doc comment for why it can still be read after a trap).
//!
//! ## Mirrors `bin/advisor.rs --save`, not the interactive REPL
//!
//! `bin/advisor.rs`'s one-shot mode (`--load`/`--save`) reads update lines,
//! applies them, advances the mirror past any opponents, then auto-plays the
//! bot's OWN top pick for every action of the resulting turn -- "there is no
//! one at a keyboard to ask, so the recommendation and the move actually
//! played are the same thing" (that file's own doc comment on `run_batch`).
//! [`run`] below is the identical sequence, reimplemented against the
//! library's public API only (`sync_to_my_turn`/`run_batch` themselves are
//! private to that binary): [`sync_to_my_turn`] is a line-for-line port of
//! its namesake, and the per-step loop in [`run`] mirrors `run_batch`'s own.
//! `"moves"` in the response is therefore the sequence of moves ACTUALLY
//! PLAYED this turn (each one was also the top recommendation at the moment
//! it was played), not a multi-choice menu -- a caller wanting to offer a
//! human alternatives at each decision point would need a different mode,
//! not implemented here (see the notes file's "does not work" section).

use std::sync::{Mutex, OnceLock};

use tta::advisor::session::{Advisor, SearchConfig, SearchMode};
use tta::advisor::state_io;
use tta::bots::plan;
use tta::bots::weighted::eval::{self, WeightedBot};
use tta::fixtures::{self, Json};
use tta::game;
use tta::human_policy;

// ------------------------------------------------------------ baked assets
//
// `include_str!` of the exact files the CLI reads from disk at runtime
// (`experiments/rust_champion_{n}p.json`, `analysis/frozen/
// human_weights.json`) -- behaviour-preserving by construction, since it is
// the same bytes going through the same parser (`eval::parse_weights` /
// `human_policy::parse_weights_text`), just read at COMPILE time instead of
// at run time. Both files are gitignored league/training output (the live
// league's own current champion, and the frozen human-imitation prior), not
// tracked in this checkout's git history -- but `include_str!` only needs
// them present on the machine that RUNS `cargo build -p ttawasm`, same as
// they need to be present for `cargo build --bin advisor` to see anything
// but the "built-in default weights" fallback today. See the notes file for
// what happens on a from-scratch clone with no league output yet.
const CHAMPION_2P: &str = include_str!("../../../experiments/rust_champion_2p.json");
const CHAMPION_3P: &str = include_str!("../../../experiments/rust_champion_3p.json");
const CHAMPION_4P: &str = include_str!("../../../experiments/rust_champion_4p.json");
const HUMAN_WEIGHTS: &str = include_str!("../../../analysis/frozen/human_weights.json");

/// The gameplay-evaluation weights the CLI's own `advisor::load_bot` would
/// pick for a fresh checkout with no `--weights` override: the baked-in
/// champion for `players`, falling back to [`WeightedBot::default`] (the
/// same "built-in default weights" string the CLI prints) if that text
/// somehow fails to parse.
fn default_bot(players: u8) -> (WeightedBot, &'static str) {
    let text = match players {
        2 => CHAMPION_2P,
        3 => CHAMPION_3P,
        _ => CHAMPION_4P,
    };
    match eval::parse_weights(text) {
        Ok(w) => (WeightedBot::new(w), "baked-in champion weights"),
        Err(_) => (WeightedBot::default(), "built-in default weights (baked champion failed to parse)"),
    }
}

/// The search mode the CLI defaults to (`--search human`, see `bin/
/// advisor.rs`'s `Args::default`): human-imitation root shortlist + beam,
/// falling back to `Greedy` exactly the way `load_search` does on a load
/// failure -- the only difference is the human weights come from
/// [`HUMAN_WEIGHTS`] (baked in at compile time) instead of a runtime read of
/// `analysis/frozen/human_weights.json`.
fn default_search() -> SearchConfig {
    match human_policy::parse_weights_text(HUMAN_WEIGHTS) {
        Ok(w) => {
            let human_weights = human_policy::vector_from_weights(&w);
            SearchConfig { mode: SearchMode::Human, plan: plan::PlanConfig::default(), human_weights }
        }
        Err(_) => SearchConfig::default(), // Greedy -- see `SearchConfig::default`'s own doc comment
    }
}

// -------------------------------------------------------------- last_error
//
// A panic hook is not a safety net that lets `advise` keep running --
// `panic = "abort"` (inherited from the workspace root's `[profile.release]`,
// shared with every other binary in this crate) means a panic anywhere below
// `advise` traps the WHOLE call, unconditionally, with no unwind. What the
// hook buys is a message a HOST can still read afterwards: a WebAssembly trap
// only unwinds the single host->wasm call that was in progress, not the
// instance's linear memory or its other exports, so a JS caller that wraps
// its `advise` call in try/catch can, after catching the trap, call
// `last_error` (a second, ordinary export) and get the panic message out of
// memory that was written just before the trap fired and was never rolled
// back. It cannot repair the interrupted call, and a caller that suspects a
// trap happened should treat the whole module instance as suspect afterwards
// (re-instantiate rather than trust further calls) -- this exists purely so
// "it crashed" comes with a reason instead of an opaque `RuntimeError:
// unreachable` in the browser console.
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
static PANIC_HOOK_INSTALLED: OnceLock<()> = OnceLock::new();

fn ensure_panic_hook() {
    PANIC_HOOK_INSTALLED.get_or_init(|| {
        std::panic::set_hook(Box::new(|info| {
            if let Ok(mut slot) = LAST_ERROR.lock() {
                *slot = Some(info.to_string());
            }
        }));
    });
}

// ---------------------------------------------------------------- C ABI
//
// See the notes file for the exact framing. `alloc`/`dealloc` are a plain
// bump-free byte-buffer allocator built directly on `Vec<u8>`: `alloc` grows
// a zeroed buffer to `len` bytes and leaks it (`mem::forget`) so the pointer
// stays valid across the wasm/JS boundary; `dealloc` reconstructs the exact
// same `Vec` (same `len` as both length AND capacity -- `alloc` never
// reallocates, so this is exact, not an approximation) and lets it drop.
// A caller MUST pass `dealloc` the same `len` it originally asked `alloc`
// for (or, for `advise`'s own return value, `4 + json_len` -- see `advise`'s
// doc comment); passing the wrong length is undefined behaviour, exactly as
// it would be for a hand-rolled allocator in any other language.

#[no_mangle]
pub extern "C" fn alloc(len: usize) -> *mut u8 {
    let mut buf = vec![0u8; len];
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: caller contract documented on this module's C-ABI section --
    // `ptr`/`len` must be a pair this crate itself hand out via `alloc` or
    // `advise`/`last_error`'s own return value, with `len` unchanged.
    unsafe {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

/// One request/response exchange. `ptr`/`len` name a UTF-8 JSON request
/// already sitting in this module's own memory (written there through a
/// buffer the caller got from [`alloc`]). Returns a pointer to `[u32
/// little-endian byte length][UTF-8 JSON response]`; the caller reads the
/// 4-byte length, reads that many bytes right after it, and then MUST call
/// [`dealloc`] on the returned pointer with length `4 + json_len` to free it
/// -- this function does not free its own return value, and does not free
/// the input buffer either (the caller allocated it and the caller frees it,
/// same convention both directions).
#[no_mangle]
pub extern "C" fn advise(ptr: *const u8, len: usize) -> *mut u8 {
    ensure_panic_hook();
    // SAFETY: caller contract on `advise` -- `ptr`/`len` describe a live,
    // initialized byte range in this module's own linear memory.
    let input = unsafe { std::slice::from_raw_parts(ptr, len) };
    let response = match std::str::from_utf8(input) {
        Ok(text) => handle_request(text),
        Err(e) => error_json(&format!("request is not valid UTF-8: {e}")),
    };
    frame(response.as_bytes())
}

/// The panic hook's last-caught message, framed the same way `advise`'s
/// response is (`[u32 LE length][UTF-8 bytes]`), empty if nothing has
/// panicked yet. See [`LAST_ERROR`]'s own doc comment for what this can and
/// cannot be used for.
#[no_mangle]
pub extern "C" fn last_error() -> *mut u8 {
    let msg = LAST_ERROR.lock().ok().and_then(|g| g.clone()).unwrap_or_default();
    frame(msg.as_bytes())
}

fn frame(payload: &[u8]) -> *mut u8 {
    let mut buf = Vec::with_capacity(4 + payload.len());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(payload);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

// ------------------------------------------------------------- the request

fn error_json(msg: &str) -> String {
    Json::obj(vec![("ok", Json::Bool(false)), ("error", Json::Str(msg.to_string()))]).to_string()
}

fn handle_request(text: &str) -> String {
    match run(text) {
        Ok(json) => json,
        Err(e) => error_json(&e),
    }
}

/// The whole exchange: parse the request, build (or resume) the mirror,
/// apply the reported update lines, auto-play my own turn, and answer with
/// the new state. See this module's top doc comment for why the shape here
/// is `bin/advisor.rs --save`'s, not the interactive REPL's.
fn run(text: &str) -> Result<String, String> {
    let doc = fixtures::parse_json(text).map_err(|e| format!("request is not valid JSON: {e:?}"))?;

    let players = doc
        .get("players")
        .and_then(Json::as_f64)
        .map(|n| n as u8)
        .ok_or_else(|| "missing or non-numeric 'players'".to_string())?;
    if !(2..=4).contains(&players) {
        return Err(format!("'players' must be 2, 3 or 4, got {players}"));
    }

    let board = match doc.get("state").and_then(Json::as_str) {
        Some(s) if !s.is_empty() => state_io::loads(s)?,
        _ => {
            let seat = doc.get("seat").and_then(Json::as_f64).map(|n| n as u8).unwrap_or(0);
            if seat >= players {
                return Err(format!("'seat' must be 0..{}, got {seat}", players - 1));
            }
            let seed = doc.get("seed").and_then(Json::as_f64).map(|n| n as u64).unwrap_or(0);
            state_io::new_board(players, seat, seed)
        }
    };

    let weights_override = match doc.get("weights") {
        None => None,
        Some(Json::Null) => None,
        Some(w @ Json::Obj(_)) => Some(eval::parse_weights(&w.to_string()).map_err(|e| format!("bad 'weights': {e}"))?),
        Some(Json::Bool(_) | Json::Num(_) | Json::Str(_) | Json::Arr(_)) => {
            return Err("'weights' must be a JSON object".to_string())
        }
    };
    let (bot, bot_source) = match weights_override {
        Some(w) => (WeightedBot::new(w), "request-supplied weights"),
        None => default_bot(players),
    };

    let lines = match doc.get("lines") {
        None => Vec::new(),
        Some(Json::Arr(items)) => {
            let mut v = Vec::with_capacity(items.len());
            for item in items {
                v.push(item.as_str().ok_or_else(|| "'lines' must be an array of strings".to_string())?.to_string());
            }
            v
        }
        Some(_) => return Err("'lines' must be an array of strings".to_string()),
    };

    let mut adv = Advisor::new(board, bot, bot_source.to_string());
    adv.search = default_search();

    let msgs = sync_to_my_turn(&mut adv, &lines.join("\n"))?;
    let mut log = String::new();
    for m in &msgs {
        log.push_str(&format!("ok: {m}\n"));
    }

    // Captured BEFORE the turn is auto-played: these are the resources the
    // moves below are about to spend, which is what the front end displays
    // next to them. Emitted as fields rather than as a line in `log` -- the
    // caller should never have to regex free text to find them.
    let position = {
        let st = adv.state();
        let p = st.actor();
        Json::obj(vec![
            ("round", Json::Num(f64::from(st.round))),
            ("age", Json::Str(format!("{:?}", st.age_civil))),
            ("civil_actions", Json::Num(f64::from(p.civil_actions))),
            ("military_actions", Json::Num(f64::from(p.military_actions))),
            ("food", Json::Num(f64::from(p.food))),
            ("resources", Json::Num(f64::from(p.resources))),
            ("science", Json::Num(f64::from(p.science))),
        ])
    };

    let mut moves = Vec::new();
    let mut step = 0;
    while adv.my_turn() && !adv.state().game_over && step < 40 {
        step += 1;
        let cands = adv.recommend(3);
        let Some(top) = cands.first() else { break };
        let text = top.text.clone();
        let score = top.score;
        let reason = top.reason.clone();
        let mv = top.mv;
        let (ok, msg) = adv.play(mv);
        if !ok {
            return Err(format!("internal error: the bot's own top pick was rejected: {msg}"));
        }
        moves.push(Json::obj(vec![("text", Json::Str(text)), ("score", Json::Num(score)), ("detail", Json::Str(reason))]));
    }
    if step == 40 {
        return Err("my turn never settled (40-action guard hit)".to_string());
    }

    if adv.state().game_over {
        let scores: Vec<String> = game::scores(adv.state()).iter().enumerate().map(|(i, s)| format!("p{i}={s}")).collect();
        log.push_str(&format!("game over.  final culture: {}\n", scores.join(", ")));
    }

    Ok(Json::obj(vec![
        ("ok", Json::Bool(true)),
        ("moves", Json::Arr(moves)),
        ("state", Json::Str(state_io::dumps(&adv.board))),
        ("position", position),
        ("log", Json::Str(log)),
    ])
    .to_string())
}

/// Port of `bin/advisor.rs`'s private `sync_to_my_turn`: apply `input`'s
/// update lines in `state_io::PatchTiming` order (an `event` line lands
/// BEFORE the advance past any opponents, everything else lands AFTER --
/// see that file's own doc comment for why applying a correction on the
/// wrong side of the advance corrupts the row), advancing the mirror past
/// any opponents in between. Reimplemented here (rather than exported from
/// the binary, which cannot be a library dependency) against `state_io`'s
/// public `patch_all`/`split_by_timing` and `Advisor`'s own public methods
/// only -- no access to anything the CLI has that this crate does not.
fn sync_to_my_turn(adv: &mut Advisor, input: &str) -> Result<Vec<String>, String> {
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
