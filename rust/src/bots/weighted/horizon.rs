//! `engine/bots/weighted.py` lines 1056-1415: the game horizon -- how many
//! rounds are left, how much of the civil supply is unseen, how "late" the
//! game is, and what a per-turn RATE feature is worth given how much game is
//! left to collect it in.
//!
//! TODO: port `_tail`, `_supply`, `_live`, `cards_unseen`, `_replenishes`,
//! `take_rate`, `rounds_left`, `lateness`, `horizon_scale`, `rate_multiplier`,
//! and the constants `AGE_IV_ROUNDS`/`_SWEEP`/`_ROW`/`_TAKE_PRIOR`/
//! `_TAKE_PRIOR_W`/`RATE_KEYS`. `_tail`/`_supply` are memo caches in Python
//! keyed by (players, age); in Rust these must be `const` tables or cheap
//! recomputation, NOT a lazily-populated global map.
