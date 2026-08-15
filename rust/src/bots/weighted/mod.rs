//! `WeightedBot`: the 1-ply evaluator the whole bot rests on
//! (`engine/bots/weighted.py`, 4514 lines -- the single biggest file left to
//! port in `engine/bots/`). Split one file per independent concern, matching
//! the Python source's own section breaks, so multiple workers can each land
//! a port in an uncontested file. Every submodule's own doc comment names
//! its exact Python line range and lists what it still owes; see those
//! before editing one.
//!
//! Landed so far: [`weights`] and [`horizon`], which have no dependency on
//! anything else in this file ([`horizon`] depends on [`weights`] for
//! [`weights::WeightKey::RateHorizon`] and [`weights::Weights`]); [`rivals`],
//! which depends on both of those plus `costs`/`effects`/`bots::counting`
//! (all landed elsewhere in the crate) but not on `evaluate` itself;
//! [`cards`], BOTH layers now -- the yield-plumbing table (`card_yields`/
//! `card_choice`/`sum_yields`/`board_credit_key`/`swap_type`/
//! `yield_marginal`/the `DELIBERATELY_UNPRICED`/`UNPRICED_VALUES` tables) AND
//! the valuation layer built on top of it (`action_value`/`tech_value`/
//! `gov_value`/`card_potential`/`hand_potential`/`wonder_potential`/
//! `hand_mil_potential`/`rival_hand_potential`/`tactic_terms`) -- see
//! [`cards`]'s own top doc comment for the exact split; [`row`], which calls
//! [`cards::card_potential`] directly (no injected dependency -- an earlier
//! revision took it as one while only `cards.rs`'s yield-plumbing layer had
//! landed, and dropped it once the valuation layer did); [`events`], the Age
//! III scoring-event modelling `features`/`row.rs`'s row terms both lean on;
//! and [`features`], `evaluate`'s raw feature vector. [`eval`] is now landed
//! too -- `evaluate` itself, `WeightedBot`, and the dominance guard
//! (`dominance_repair`/`NET_NONNEG_PHASE`/`DOMINATES`/`weights::WeightKey::
//! sign_intent`) -- and
//! is the module every other one in this file exists to feed; see its own
//! top doc comment for the handful of Python-only mechanisms (the journalled
//! search path, a per-candidate exception guard, `load_weights`/
//! `save_weights`) it deliberately does not carry, and why. This was the last
//! unported piece of `engine/bots/weighted.py`. [`registry`] is not a port of
//! anything -- it is this crate's replacement for the retired Python
//! `tests/test_coordinate_registry.py` (docs/OPEN_ITEMS.md item 5), a
//! `#[cfg(test)]`-only module with no production code of its own.

pub mod cards;
pub mod eval;
pub mod events;
pub mod features;
pub mod horizon;
pub mod registry;
pub mod rivals;
pub mod row;
pub mod weights;
