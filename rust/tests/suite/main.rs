//! Single integration-test binary for the whole `rust/tests/` suite.
//!
//! Cargo builds one release-optimised, `lto = "fat"`, `codegen-units = 1`
//! executable per top-level file under `tests/`. With 16-17 top-level files
//! that meant 16-17 full library link-and-optimise passes for one `cargo test
//! --release`, dominating both a cold build and -- worse -- every incremental
//! rebuild after touching a single source file, since each of those binaries
//! re-links and re-optimises the whole ~30k-line library from scratch. Nothing
//! is a top-level file under `tests/` anymore; this file (`tests/suite/
//! main.rs`) is Cargo's other supported integration-test convention -- a
//! `<name>/main.rs` pair names a single target `<name>` the same way `tests/
//! <name>.rs` would, but its siblings resolve as plain submodules of THIS
//! directory instead of each being auto-discovered as their own target.
//! Everything that used to be its own top-level file is now one such module,
//! so there is one link-and-optimise pass instead of many. No test's
//! assertions changed in that move -- see the commit that introduced this
//! file for the measured before/after.
//!
//! Run the whole suite with `cargo test --release --test suite`, or a single
//! module with e.g. `cargo test --release --test suite -- differential::`.

mod common;

mod bench_playout;
mod board_yields;
mod random_game;
