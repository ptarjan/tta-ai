//! The `REPLAY_DEBUG*` switches, read from the environment exactly once.
//!
//! # Why this module exists
//!
//! These flags are tested from inside the production, scoring and cost loops
//! -- the innermost code in the engine, run millions of times per climb
//! generation. Written as `std::env::var("REPLAY_DEBUG_ALL").is_ok()` at each
//! of those sites, every test did three avoidable things:
//!
//! 1. took a **process-wide lock**. `getenv` on macOS serialises on the
//!    environment (`__findenv_locked`), so two engine threads stepping through
//!    production contend with each other for a string that cannot change.
//!    A `sample` of the live 2p climb arm showed that lock in the profile.
//! 2. linearly scanned `environ` comparing strings, per call.
//! 3. allocated an `OsString`/`String` whenever the variable *was* set, which
//!    is the debugging path where the allocation is least affordable.
//!
//! # Why reading once is correct, and not a lazily-populated cache
//!
//! The forbidden thing is a global that memoises a *computed* value, because
//! then the cache and the truth can drift. Nothing here is computed: a
//! process's environment is fixed for its lifetime as far as this engine is
//! concerned (no code in this crate calls `set_var`, and the replay tools are
//! run as `REPLAY_DEBUG_ALL=1 replaystats ...`). The flag IS the value, so
//! there is no second source of truth to disagree with.

use std::sync::LazyLock;

/// Reads `name` once, treating "set to anything at all" as on -- the exact
/// predicate the call sites used (`std::env::var(..).is_ok()`), so switching
/// them over cannot change which games print debug output.
fn flag(name: &str) -> bool {
    std::env::var(name).is_ok()
}

static REPLAY_DEBUG: LazyLock<bool> = LazyLock::new(|| flag("REPLAY_DEBUG"));
static REPLAY_DEBUG_ALL: LazyLock<bool> = LazyLock::new(|| flag("REPLAY_DEBUG_ALL"));
static REPLAY_DEBUG_TECHS: LazyLock<bool> = LazyLock::new(|| flag("REPLAY_DEBUG_TECHS"));

/// `REPLAY_DEBUG`: per-game replay tracing.
pub fn replay_debug() -> bool {
    *REPLAY_DEBUG
}

/// `REPLAY_DEBUG_ALL`: the firehose -- every production step, every award.
/// This is the one tested from the hot loops, and the reason this module is
/// not just a nicety.
pub fn replay_debug_all() -> bool {
    *REPLAY_DEBUG_ALL
}

/// `REPLAY_DEBUG_TECHS`: technology-progress tracing specifically.
pub fn replay_debug_techs() -> bool {
    *REPLAY_DEBUG_TECHS
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of the module is that the answer is stable, so a caller
    /// in a hot loop can be trusted not to re-derive it. If these ever stopped
    /// agreeing across calls, the flags would have become the drifting cache
    /// this module's doc comment promises they are not.
    #[test]
    fn a_flag_reads_the_same_every_time_it_is_asked() {
        for _ in 0..3 {
            assert_eq!(replay_debug(), replay_debug());
            assert_eq!(replay_debug_all(), replay_debug_all());
            assert_eq!(replay_debug_techs(), replay_debug_techs());
        }
    }

    /// Deleting the `env::var` calls once is worth little if the next edit
    /// puts one back into a production loop, which is exactly how this cost
    /// arrived in the first place -- it is invisible at the call site and
    /// reads as free. So the ban is enforced on the source itself rather than
    /// left as a convention: engine files must go through this module.
    ///
    /// `src/bin/` is deliberately exempt -- those are command lines, read once
    /// at startup, where `env::var` is the right tool and costs nothing.
    #[test]
    fn no_engine_file_reads_a_debug_flag_from_the_environment_directly() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut offenders = Vec::new();
        let mut stack = vec![src.clone()];
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir).expect("src/ is readable");
            for entry in entries {
                let path = entry.expect("a readable dir entry").path();
                if path.is_dir() {
                    // `src/bin/` holds command lines, not engine code.
                    if path.file_name().is_some_and(|n| n == "bin") {
                        continue;
                    }
                    stack.push(path);
                    continue;
                }
                if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| n != "debugflags.rs")
                {
                    let text = std::fs::read_to_string(&path).expect("a readable .rs file");
                    if text.contains("env::var(\"REPLAY_DEBUG") {
                        offenders.push(path.strip_prefix(&src).unwrap_or(&path).display().to_string());
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "these files read a REPLAY_DEBUG* flag from the environment directly, which takes a \
             process-wide lock every time the surrounding loop runs -- call \
             `debugflags::replay_debug_all()` (etc.) instead: {offenders:?}"
        );
    }
}
