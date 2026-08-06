//! `WeightedBot`: the 1-ply evaluator the whole bot rests on
//! (`engine/bots/weighted.py`, 4514 lines -- the single biggest file left to
//! port in `engine/bots/`). Split one file per independent concern, matching
//! the Python source's own section breaks, so multiple workers can each land
//! a port in an uncontested file. Every submodule's own doc comment names
//! its exact Python line range and lists what it still owes; see those
//! before editing one.
//!
//! All eight submodules below are stubs for now: a doc comment naming their
//! Python range and a TODO list, deliberately with no code in them yet, so a
//! later worker's first edit to one is not a merge conflict with this
//! commit.

pub mod cards;
pub mod eval;
pub mod events;
pub mod features;
pub mod horizon;
pub mod rivals;
pub mod row;
pub mod weights;
