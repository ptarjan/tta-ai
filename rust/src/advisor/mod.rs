//! The advisor: a text co-pilot for a human playing Through the Ages at a
//! physical table. `state_io` is the serialization / mirror-editing layer;
//! the ranking + REPL layer is ported on top of it separately.

pub mod state_io;
