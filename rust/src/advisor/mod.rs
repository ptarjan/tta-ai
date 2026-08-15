//! The advisor: a text co-pilot for a human playing Through the Ages at a
//! physical table. `state_io` is the serialization / mirror-editing layer;
//! `describe` turns a move (or a feature-vector delta) into plain English;
//! `advisor` is the mirror + bot + session operations the REPL in
//! `bin/advisor.rs` drives.

pub mod session;
pub mod describe;
pub mod state_io;

pub use session::Advisor;
