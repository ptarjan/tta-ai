//! The bot layer: what a player's board is worth, and what a bot does with
//! that.
//!
//! Ports `engine/bots/` (~10k lines of Python). [`board_yields`] is the first
//! module landed -- pure computation over [`crate::state::GameState`]/
//! [`crate::state::PlayerState`], no search, no weights, no RNG, which is
//! what makes it safe to port and verify on its own before the modules that
//! sit on top of it (`weighted.py`'s evaluator, the search itself) exist here.

pub mod board_yields;
