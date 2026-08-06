//! `WeightedBot`: the 1-ply evaluator the whole bot rests on
//! (`engine/bots/weighted.py`, 4514 lines -- the single biggest file left to
//! port in `engine/bots/`). Split one file per independent concern, matching
//! the Python source's own section breaks, so multiple workers can each land
//! a port in an uncontested file. Every submodule's own doc comment names
//! its exact Python line range and lists what it still owes; see those
//! before editing one.
//!
//! Four submodules are ported so far: [`weights`] and [`horizon`], which
//! have no dependency on anything else in this file ([`horizon`] depends on
//! [`weights`] for [`weights::WeightKey::RateHorizon`] and
//! [`weights::Weights`]); [`rivals`], which depends on both of those plus
//! `costs`/`effects`/`bots::counting` (all landed elsewhere in the crate) but
//! not on `features`/`evaluate`/the card pricing machinery; and [`cards`],
//! the YIELD-PLUMBING layer only (`_card_yields`/`_card_choices`/
//! `_sum_yields`/`_board_credit_key`/`_swap_type`/`_yield_marginal` and the
//! `_*_TO_FEATURE`/`DELIBERATELY_UNPRICED` tables) -- the VALUATION layer
//! built on top of it (`action_value`/`tech_value`/`gov_value`/
//! `card_potential`/`hand_potential`/etc.) is still unlanded; see
//! [`cards`]'s own top doc comment for the exact split and the API it
//! exposes for that layer to call. Every other submodule below is a stub: a
//! doc comment naming its Python range and a TODO list, deliberately with no
//! code in it yet, so a later worker's first edit to it is not a merge
//! conflict with this commit.

pub mod cards;
pub mod eval;
pub mod events;
pub mod features;
pub mod horizon;
pub mod rivals;
pub mod row;
pub mod weights;
