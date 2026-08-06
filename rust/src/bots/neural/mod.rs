//! The neural stack: a value network scoring a flat board encoding, and the
//! two search bots built on it. Ports `engine/bots/neural_encode.py` (419
//! lines), `engine/bots/neural_net.py` (152), `engine/bots/neural_plan.py`
//! (332) and `engine/bots/neural_bot.py` (98) -- the last bot code left in
//! Python. Split one file per Python module, matching `bots::weighted`'s own
//! precedent.
//!
//! * [`encode`] -- `neural_encode.py`: the torch-free flat-`Vec<f64>` board
//!   encoding every other module here feeds. Reuses `bots::weighted::
//!   features::sweep_tableau`, `costs::row_cost` and `combat::pacts_for`
//!   rather than restating them; see its own top doc comment.
//! * [`net`] -- `neural_net.py`: the value network's forward-pass ARITHMETIC,
//!   plus this crate's own hand-rolled checkpoint format (`Cargo.toml`'s
//!   `[dependencies]` is deliberately empty -- no torch, no ndarray, no
//!   serde). See its own top doc comment for why this module has no Python
//!   oracle to differential-test against at all (torch is unavailable both
//!   to the Python module on its host machine and to this
//!   differential-testing environment).
//! * [`rankdata`] -- `experiments/neural_rankdata.py`: training-data
//!   generation from Rust self-play (ranking pairs + value anchors), and
//!   the `Shard` binary format [`train`] reads.
//! * [`train`] -- `experiments/neural_train_rank.py`: hand-rolled
//!   backpropagation, AdamW, and the combined value+ranking training
//!   objective for [`net::ValueNet`], pinned against finite differences
//!   (there is no Python oracle here either, for the same reason as
//!   `net`). See its own top doc comment for the loss shape; the training
//!   data it consumes is [`rankdata::Shard`].
//! * [`bot`] -- `neural_bot.py`: `NeuralBot`, the 1-ply search built on
//!   [`net::ValueNet`].
//! * [`spec`] -- `experiments/arena.py::load_spec`/`make_bot`: the one
//!   bot-spec grammar (`plan:champion_2p.json,width=8`,
//!   `nplan:best_search.ckpt`) that can name a CHECKPOINT as a player, which
//!   `bots::greedy::make_seats`'s bare-`BotKind` grammar cannot.
//! * [`eval`] -- `experiments/neural_eval.py`: seat-rotated head-to-head
//!   play strength between any two [`spec`] contenders, which is the only
//!   promotion criterion `docs/NEURAL_EVAL.md` trusts.
//! * [`plan`] -- `neural_plan.py`: `NeuralPlanBot`, the whole-turn beam
//!   search built on it. Reuses `bots::plan`'s determinize/quiesce/pending
//!   machinery rather than forking a second copy; see its own top doc
//!   comment for the one place the two beams genuinely differ (batched
//!   per-ply scoring, not per-candidate).

pub mod bot;
pub mod encode;
pub mod eval;
pub mod net;
pub mod plan;
pub mod rankdata;
pub mod spec;
pub mod train;
