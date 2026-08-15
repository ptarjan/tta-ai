//! Diagnostic-only thread-local `(game id, journal line)` label for the
//! `TIE_CENSUS` superlative-selection dump (see `events.rs`'s
//! `apply_single_target`/`conditional_target`/`resolve_count_targets`, and
//! `debugflags::tie_census`). Replay is single-game, single-threaded per
//! process (`replaystats`'s own `for meta in &games` loop, one `replay_game`
//! call at a time), so a thread-local written once per game and once per
//! journal line, and read back a few call-frames deeper inside `events.rs`
//! (which has no journal/game-id of its own to thread through every
//! function signature), is a label -- not a second source of truth for any
//! engine decision. Nothing here is read by production/self-play code; only
//! the `TIE_CENSUS`-gated print helpers consume it.

use std::cell::{Cell, RefCell};

thread_local! {
    static GAME_ID: RefCell<String> = const { RefCell::new(String::new()) };
    static LINENO: Cell<usize> = const { Cell::new(0) };
    static CARD: RefCell<&'static str> = const { RefCell::new("") };
}

/// Set once per game, from `replay_common::replay_game`'s own `meta.id`.
pub fn set_game(id: &str) {
    GAME_ID.with(|g| {
        let mut g = g.borrow_mut();
        g.clear();
        g.push_str(id);
    });
}

/// Set once per journal line consumed, alongside `Replayer::current_lineno`.
pub fn set_lineno(n: usize) {
    LINENO.with(|l| l.set(n));
}

/// The game id most recently set by [`set_game`], or `""` before the first
/// call this process (never happens under `replaystats`/`replay`, which set
/// it before touching any journal line).
pub fn game_id() -> String {
    GAME_ID.with(|g| g.borrow().clone())
}

/// The journal line most recently set by [`set_lineno`].
pub fn lineno() -> usize {
    LINENO.with(|l| l.get())
}

/// Set once per `events::resolve_event` call, from that card's own `name`
/// (`'static`, straight off the card table -- no allocation needed).
pub fn set_card(name: &'static str) {
    CARD.with(|c| *c.borrow_mut() = name);
}

/// The card name most recently set by [`set_card`].
pub fn card_name() -> &'static str {
    CARD.with(|c| *c.borrow())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of this module: a value set from one call survives to
    /// be read back by an unrelated later call, on the same thread -- the
    /// exact relay `events.rs`'s census print depends on.
    #[test]
    fn a_value_set_here_is_read_back_by_a_later_unrelated_call() {
        set_game("7521544");
        set_lineno(339);
        assert_eq!(game_id(), "7521544");
        assert_eq!(lineno(), 339);
        set_game("7522113");
        set_lineno(42);
        assert_eq!(game_id(), "7522113");
        assert_eq!(lineno(), 42);
    }
}
