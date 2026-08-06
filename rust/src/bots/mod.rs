//! The bot layer: what a player's board is worth, and what a bot does with
//! that.
//!
//! Ports `engine/bots/` (~10k lines of Python). [`board_yields`] is the first
//! module landed -- pure computation over [`crate::state::GameState`]/
//! [`crate::state::PlayerState`], no search, no weights, no RNG, which is
//! what makes it safe to port and verify on its own before the modules that
//! sit on top of it (`weighted.py`'s evaluator, the search itself) exist here.
//!
//! ## `engine/bots/fastcopy.py` (329 lines): NOT ported, deliberately
//!
//! Python's `fastcopy.copy_state` is a hand-rolled, code-generated
//! replacement for `copy.deepcopy` on a `GameState` -- its own module doc
//! comment opens with the measurement that motivates it: "`copy.deepcopy`
//! accounts for ~78% of a lookahead bot's runtime". Every trick in that file
//! (per-class generated copiers, an atomic-container fast path, a
//! private-attribute skip list) exists to avoid `deepcopy`'s per-object
//! bookkeeping -- the memo dict, the alive-list, reconstruction through
//! `__reduce_ex__` -- none of which a Rust struct copy has in the first
//! place.
//!
//! [`crate::state::GameState`] already derives `Clone`, and every field it
//! owns is a fixed-size array / scalar / small enum
//! (`CardList<N>`/`[T; N]`/`u8`/`Option<T>`...), never a `Vec`, a `HashMap`
//! or a trait object -- DESIGN.md's rule against heap allocation in hot
//! state is exactly what makes the derived `Clone` a flat memcpy-shaped
//! copy, no allocation, no indirection to chase. That derived `Clone` IS
//! this module's "fast structural copy of a GameState for 1-ply search",
//! not a stand-in for one -- `economy.rs`'s own doc comment already reached
//! this conclusion independently, for the journal rather than the copier:
//! "There is no `journal.touch` equivalent: Python's journal is an undo-log
//! for `GreedyBot`'s trial moves (`engine/journal.py`) that exists ... trial
//! moves just clone the state and there is nothing to journal."
//!
//! Two fields Python's copier special-cases do not exist here to special-case:
//!
//! * **`GameState.log`** (Python: a `list` of up to ~400 strings, dropped by
//!   `copy_state` by default because "search never reads it" and it is "the
//!   single biggest copy cost") -- [`crate::state::GameState`] has no `log`
//!   field at all. Nothing in this port writes a play-by-play log onto the
//!   state, so there is nothing to drop.
//! * **`state._stats_cache`** (Python: a private cache `fastcopy` strips
//!   because caches are not state) -- `effects.rs`'s own doc comment records
//!   the equivalent Rust decision explicitly: `Stats::compute` is
//!   recomputed fresh on every call, "DELIBERATELY not replicated
//!   [cached] here ... there is no hot loop a cache is fixing". No cache
//!   field exists on `GameState` to strip.
//!
//! So `fastcopy.py`'s three jobs -- copy fast, drop `log`, drop the cache --
//! are respectively: what `#[derive(Clone)]` already does structurally, a
//! field this port never created, and a field this port never created. There
//! is no semantics left over to port: nothing in `fastcopy.py` encodes a
//! shallow/deep-copy distinction that changes a bot's behaviour (the
//! generic-vs-generated split, the atomic-container registry and paranoid
//! mode are all pure performance machinery over CPython's data model, guarded
//! by paranoid-mode assertions that exist only because Python has no type
//! system to make the atomic-container claim by construction the way Rust's
//! field types already do). `keep_log`, `copy_state`'s only behavioural
//! parameter, has no Rust counterpart to receive it because there is no
//! `log` field for it to gate.
//!
//! ## `engine/bots/trial.py` (78 lines): NOT ported, deliberately
//!
//! `trial.py` is machinery shared by every Python search bot (`GreedyBot`,
//! `WeightedBot`, `QuiescentBot`, `PlanBot`) for one job: hand each trial
//! `actions.apply(state, mv, rng)` call a *freshly seeded* `Random(0)`, so
//! every candidate move sees the identical rng stream from the identical
//! starting point -- otherwise which move a search considered FIRST would
//! perturb the shuffle every later candidate saw, and comparing their scores
//! would not be comparing the same position. It carries two things:
//!
//! * `USE_JOURNAL` -- an opt-in (`TTA_JOURNAL=1`) switch to an undo-stack
//!   search (`engine/journal.py`) instead of `fastcopy.copy_state`. There is
//!   no `journal.rs` in this port to switch to: `board_yields.rs`'s own doc
//!   comment (quoting `economy.rs`) already closed this -- "Python's journal
//!   is an undo-log for `GreedyBot`'s trial moves ... trial moves just clone
//!   the state and there is nothing to journal." One search mechanism
//!   exists here (`Clone`), so there is nothing left to toggle between.
//! * `TrialRandom`/`TRIAL_RNG`/`fresh_trial_rng` -- a `random.Random`
//!   subclass that records whether it was drawn from, plus a shared pooled
//!   instance `fresh_trial_rng()` resets to a pristine `Random(0)` ONLY when
//!   the previous consumer actually drew from it. The module's own comment
//!   gives the reason: reseeding a Mersenne Twister costs ~9us and profiled
//!   at "~6% of GreedyBot's runtime ... ~10.8% of a profile overall", so
//!   Python avoids paying it on the ~99.6% of candidates that never draw.
//!   Worked through carefully (`quiescent.py::_fresh` and `trial.py::
//!   fresh_trial_rng` are the same trick twice): the object returned is
//!   ALWAYS, at the moment of return, bit-identical to a freshly constructed
//!   `Random(0)`, regardless of pool slot or call history -- the "reset iff
//!   used" rule is exactly the condition under which the untouched object
//!   already equals a fresh one. So the pool has zero OBSERVABLE effect
//!   beyond "give the caller `Random(0)`, cheaply"; it is pure CPython
//!   reseed-cost avoidance, the same shape of hack as `fastcopy.py` above.
//!
//!   It also has NOTHING to receive it here for a stronger reason than
//!   "cheap to reconstruct": [`crate::apply::apply`] **takes no rng
//!   parameter at all**. `game.rs`'s own "Randomness" doc section records
//!   why: the port derives one deterministically from `state.seed`/
//!   `state.turn`/`state.round` ([`crate::game::rng_for`]) rather than
//!   threading a caller-supplied stream through `apply`, specifically
//!   because "a second entry point taking an `&mut PyRandom` would be a
//!   second shuffle order for the same game" -- the exact "two registries
//!   that can disagree" bug class DESIGN.md exists to close. A trial clone
//!   (`state.clone()`) carries its `seed`/`turn`/`round` fields unchanged
//!   from the root, so [`crate::game::rng_for`] already gives every
//!   candidate the identical derived stream Python's pool exists to
//!   fabricate by hand -- for free, by construction, with no pool, no
//!   "used" flag and no reset to write. `bots/quiescent.rs` calls
//!   [`crate::apply::apply`] directly and needs no rng parameter to do it.

pub mod board_yields;
pub mod book;
pub mod counting;
pub mod pending;
pub mod quiescent;
pub mod weighted;
