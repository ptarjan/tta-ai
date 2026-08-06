//! GPU training for `tta`'s value net -- candle autograd on top of the SAME
//! network `rust/src/bots/neural/net.rs` defines and the bots load at play
//! time.
//!
//! ## Why this crate exists, and why it is separate from `tta`
//!
//! `rust/Cargo.toml`'s `[dependencies]` is deliberately empty (see that
//! file's own header comment and `rust/DESIGN.md` rule 2): the core engine
//! parses no JSON, allocates nothing at start-up, builds anywhere `rustc`
//! runs with no install step, and no library update can quietly change how
//! the game plays. That property is worth protecting.
//!
//! GPU training needs autograd and a CUDA backend, and hand-writing either
//! is not a serious option -- a Rust programmer reaches for an ML crate
//! here, the same way `net.rs`'s forward pass reaches for ordinary `f64`
//! arithmetic rather than a hand-rolled BLAS. So this is a SEPARATE
//! workspace member (`rust/trainer/`, added to `rust/Cargo.toml`'s
//! `[workspace] members`) that takes a path dependency on the core `tta`
//! crate and carries the ML dependency itself. `cargo test` run from
//! `rust/` (the core crate's own directory, with no `-p`/`--workspace`)
//! still builds and tests only `tta`, pulling in none of this crate's
//! dependencies -- see `rust/Cargo.toml`'s own comment on exactly that.
//! Authorised exception, not a precedent: see `rust/DESIGN.md`'s "GPU
//! training" section for Paul's 2026-08-06 decision and the reasoning.
//!
//! ## The one property that matters most here
//!
//! There are now two implementations of the value network's forward pass:
//! `net.rs`'s hand-rolled one (what bots run at play time, on any machine)
//! and [`net::GpuValueNet`] (what trains the weights those bots load). Two
//! implementations of the same maths with nothing checking they agree is
//! this project's own named recurring bug class (`rust/DESIGN.md`: "present
//! in this registry, absent from that one, and nothing fails when they
//! disagree"). [`net`]'s `#[cfg(test)]` block is the check -- read it before
//! trusting anything trained through this crate.

pub mod net;
pub mod train;
