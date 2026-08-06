//! `engine/bots/weighted.py` lines 3548-4103: the weight vector `evaluate`
//! is linear over -- `BASE_WEIGHTS`, `_PHASE_PRIOR`, `PHASE_WEIGHTS`,
//! `DEFAULT_WEIGHTS`, `PHASE_KEYS`, `RETIRED_KEYS`.
//!
//! TODO: port the table as an enum of weight keys with a const array of
//! defaults (not a string-keyed map -- see this file's eventual top doc
//! comment for why), keeping `PHASE_KEYS`' early/late generation honest and
//! `RETIRED_KEYS` distinguishable rather than silently dropped.
