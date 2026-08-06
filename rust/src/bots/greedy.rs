//! Ports `engine/bots/__init__.py` (258 lines) in full: [`RandomBot`], the
//! frozen [`GreedyKey`] weight table and [`features`]/[`evaluate`],
//! [`GreedyBot`], and the `make_bots` string registry (here [`BotKind`] +
//! [`Bot`] + [`make_bots`]). This is the last module of `engine/bots/` with
//! real gameplay content -- everything else there is already ported
//! (`board_yields.py`, `weighted.py`, `quiescent.py`, `plan.py`, `book.py`,
//! `counting.py`, the neural family) or deliberately not (`fastcopy.py`,
//! `trial.py`, `journal.py` -- see `bots/mod.rs`'s own doc comment).
//!
//! ## `WEIGHTS`/[`GreedyKey`]: its own small vocabulary, not `weighted::WeightKey`
//!
//! Read the Python source's long comment immediately above `WEIGHTS` first
//! (`engine/bots/__init__.py:60-79`); it is the design rationale and is not
//! restated here in full. Short version: `GreedyBot` is the fingerprint
//! CONTROL for this project's gate arms (two of the eight, NARROW and WIDE,
//! are `GreedyBot` and nothing else) -- its whole job is to hold still while
//! evaluator changes move the other six, so its 19 weights are FROZEN ON
//! PURPOSE and must never be "synced" with `weighted::weights::WeightKey`
//! (133 keys, a dozen shared names with genuinely different values --
//! `culture_rate` is 6.0 here, 5.0 there -- and no promise the two vocabularies
//! agree on anything beyond spelling). [`GreedyKey`] is therefore a second,
//! separate fieldless enum, built the same way `weighted::weights::WeightKey`
//! is (one match per concern: [`GreedyKey::name`]/[`GreedyKey::default_weight`]),
//! but never referencing that module's enum, its `Weights` type, or its
//! `evaluate`. A misspelled or renamed key here is a Rust compile error, the
//! identical guarantee `weighted::weights`'s own top doc comment gives for
//! the reason it gives (a `HashMap<String, f64>` would be the same shape with
//! an allocation and a hash on the hot path of every 1-ply search candidate).
//!
//! ## `features`/`evaluate`
//!
//! Straight port of `features(state, p)`/`evaluate(state, idx, weights=None)`.
//! `workers` (sum of every tech's workers, unfiltered) and the pre-government
//! half of `tech_levels` (summed over exactly `WORKER_TYPES | {"special-tech"}`
//! -- `URBAN_TYPES | UNIT_TYPES | PRODUCTION_TYPES` plus special techs) are
//! not recomputed here: [`super::weighted::features::sweep_tableau`] already
//! walks `p.techs` once and reports exactly these two numbers (its own doc
//! comment: "shared between `features` and `bots::neural::encode::
//! player_block`... this port closes that duplication instead of restating
//! it a second time"), because `engine/bots/weighted.py::features()` filters
//! and sums `tech_levels` by the identical `WORKER_TYPES` set this module's
//! Python does. Reusing it here closes the SAME duplication a third time
//! rather than writing a third copy of the tableau walk.
//!
//! `math.fsum`, not `sum()`, in Python: a CPython >=3.12-vs-PyPy 3.11
//! summation-order artifact (Neumaier-compensated vs naive float addition),
//! not a rule -- see that line's own comment in `__init__.py`. This port has
//! exactly one Rust implementation to run on, so there is no second
//! interpreter's rounding to reconcile against: [`evaluate`] below sums plain
//! `f64` in [`GreedyKey::ALL`]'s fixed declaration order (which mirrors the
//! insertion order of Python's `features()` dict literal) and that is the one
//! answer this port ever computes. The `fsum` note does not apply here.
//!
//! ## `GreedyBot`: what this port drops from `pick`/`_pick_journalled`
//!
//! Mirrors `weighted::eval::WeightedBot::choose`'s own documented choices,
//! restated here because `bots/__init__.py`'s `_pick_journalled`/
//! `USE_JOURNAL` are about to stop existing anywhere to read:
//!
//! 1. **`_pick_journalled` and the `USE_JOURNAL` branch that calls it.** See
//!    `bots/mod.rs`'s `trial.py` section -- no `journal.rs` exists in this
//!    port to switch to, so there is nothing left to toggle between; only the
//!    copy-shaped half of `pick` is ported.
//! 2. **The per-candidate `try: actions.apply(...) except Exception:
//!    continue`.** [`crate::apply::apply`] panics on an invariant violation
//!    instead of being caught, matching every other search bot in this crate.
//! 3. **`self.rng`/`seed`/`name`.** `pick` never reads `self.rng` (there is no
//!    `rng.choice(moves)` fallback in `GreedyBot.pick` at all -- unlike
//!    `WeightedBot`'s, which point 2 above makes merely unreachable, this one
//!    was never wired up in the first place) and nothing in this crate reads
//!    a bot's `name`. Dropped for the same "no unread field" reason
//!    `book.rs`/`weighted::eval::WeightedBot` already established.
//! 4. **`ALLOW_RESIGN`.** A Python CLASS attribute, not a constructor
//!    parameter, and never overridden anywhere in this repo (grepped) -- so
//!    unlike `WeightedBot.allow_resign` (a real, used constructor kwarg
//!    ported as a struct field), this is ported as [`GreedyBot::choose`]'s
//!    unconditional filtering, not a field nothing would ever set to `true`.
//!
//! ## `make_bots`: [`BotKind`]/[`Bot`]
//!
//! Python's `make_bots(spec, num_players, seed=0)` splits `spec` on commas,
//! cycles the kinds round-robin over `num_players`, and does an untyped
//! string `if/elif` -- crucially, an unrecognized kind string is not an
//! error: it falls through to the `else` branch and silently becomes a
//! `GreedyBot`. [`BotKind::from_str`] is deliberately stricter: an
//! unrecognized name is a parse error. This is a registry-robustness choice,
//! not a board-game rule, so it is made on the Rust side only -- nothing
//! compares `make_bots`'s OWN behaviour between the two engines (no
//! differential test reads it; every existing Python caller passes one of
//! the five literal, correctly-spelled kind strings), so there is no oracle
//! this could disagree with, and a typo silently becoming a different bot's
//! evaluator is a worse failure mode for a caller than a hard error.
//!
//! `num_players * 131 + i`-shaped seeds mirror Python's `random.Random(seed *
//! 131 + i)` per player; only `BotKind::Random` and `BotKind::Plan` (which
//! seeds [`crate::rng::PyRandom`] for `plan::determinize`) actually read
//! theirs, for the identical reason `GreedyBot`/`WeightedBot`/`QuiescentBot`'s
//! Python `rng` constructor arguments go unread by `pick` -- see point 3
//! above. `BotKind::Plan` overrides `PlanConfig::width` to `2` rather than
//! `plan.rs`'s own default of `8`: Python's `make_bots` passes `width=2`
//! explicitly, "as `experiments/run_league.sh` trains it and as
//! `engine/perf_check.py`'s plan cases play it" (that comment, verbatim, from
//! `__init__.py:252-254`).

use std::str::FromStr;

use crate::apply;
use crate::economy;
use crate::effects;
use crate::legal;
use crate::moves::Move;
use crate::rng::PyRandom;
use crate::state::GameState;

use super::pending;
use super::plan;
use super::quiescent;
use super::weighted;
use super::weighted::features::sweep_tableau;

// ------------------------------------------------------------- RandomBot

/// A uniform "pick one of N" primitive for [`RandomBot`], deliberately NOT
/// [`crate::rng::PyRandom`] -- that module's own top doc comment lists
/// `choice` among the CPython `random.Random` surface explicitly NOT ported
/// ("do not add more surface than [`shuffle`] needs"). `RandomBot` is "the
/// legality fuzzer" (this file's own top doc comment): a real, separate need
/// from shuffling a deck, and Paul's ruling for this whole port is "I don't
/// care about bit identical bits" -- nothing about which legal move the
/// fuzzer picks needs to replay CPython's `random.Random.choice` stream bit
/// for bit. `rust/tests/common/mod.rs::Rng` already made this exact call, for
/// the exact same reason (picking a *test driver's* moves, "which no Python
/// run shares"): SplitMix64, eight lines, no dependency. This is that same
/// shape, restated here because `tests/common` is not reachable from `src/`.
#[derive(Debug)]
struct BotRng(u64);

impl BotRng {
    fn new(seed: u64) -> BotRng {
        BotRng(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Uniform over legal moves -- "the legality fuzzer" (this file's top doc
/// comment). Mirrors `RandomBot(rng=None, seed=None, allow_resign=False)`;
/// `rng`/`seed` collapse to [`BotRng::new`] since this port has one RNG shape
/// for this bot, not Python's "caller-supplied or fresh `Random(seed)`"
/// choice.
#[derive(Debug)]
pub struct RandomBot {
    rng: BotRng,
    /// Resigning (RULES_SPEC 5.11) is legal on almost every turn; a
    /// uniform-random bot would end most games in round 2 if it could pick
    /// `Move::Resign` like any other move, so it is filtered out by default
    /// -- opt in with [`RandomBot::allow_resign`] set `true`.
    pub allow_resign: bool,
}

impl RandomBot {
    pub fn new(seed: u64) -> RandomBot {
        RandomBot { rng: BotRng::new(seed), allow_resign: false }
    }

    /// `choose(state, moves, rng=None)`: uniform over `moves`, `Move::Resign`
    /// filtered out first unless [`Self::allow_resign`] -- exactly
    /// `live = [m for m in moves if m[0] != "resign"]; moves = live or
    /// moves`. `rng` is always `self.rng` here (Python's `rng or self.rng`
    /// caller override has no real caller in this crate to serve).
    ///
    /// # Panics
    /// If `moves` is empty (a caller bug, matching `random.choice([])`'s own
    /// `IndexError` and every other bot in this port).
    pub fn choose(&mut self, moves: &[Move]) -> Move {
        let filtered = super::filter_resign(moves, self.allow_resign);
        let pool: &[Move] = filtered.as_slice();
        let i = self.rng.below(pool.len());
        pool[i]
    }

    /// `__call__`: generates `moves` itself via [`legal::legal_moves`], the
    /// same split every other bot in this crate keeps between move
    /// GENERATION and SELECTION.
    pub fn pick(&mut self, state: &GameState) -> Move {
        let moves = legal::legal_moves(state);
        self.choose(moves.as_slice())
    }
}

// ------------------------------------------------------------ evaluation

/// One coordinate of [`GreedyBot`]'s evaluator. See this module's top doc
/// comment for why this is a SEPARATE enum from
/// `weighted::weights::WeightKey` rather than a shared one. `#[repr(usize)]`
/// with no explicit discriminants assigns 0..N-1 in declaration order, the
/// same trick `weighted::weights::WeightKey` uses to make `self as usize` a
/// valid array index.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(usize)]
pub enum GreedyKey {
    Culture,
    CultureRate,
    ScienceRate,
    Science,
    Strength,
    FoodRate,
    ResourceRate,
    Food,
    Resources,
    Workers,
    FreeWorkers,
    YellowBank,
    CivilActions,
    MilitaryActions,
    HappyMargin,
    Wonders,
    WonderProgress,
    Hand,
    TechLevels,
}

impl GreedyKey {
    /// Every key, in the same order Python's `WEIGHTS`/`features()` dict
    /// literals write them -- the order [`evaluate`] sums in.
    pub const ALL: &'static [GreedyKey] = &[
        GreedyKey::Culture,
        GreedyKey::CultureRate,
        GreedyKey::ScienceRate,
        GreedyKey::Science,
        GreedyKey::Strength,
        GreedyKey::FoodRate,
        GreedyKey::ResourceRate,
        GreedyKey::Food,
        GreedyKey::Resources,
        GreedyKey::Workers,
        GreedyKey::FreeWorkers,
        GreedyKey::YellowBank,
        GreedyKey::CivilActions,
        GreedyKey::MilitaryActions,
        GreedyKey::HappyMargin,
        GreedyKey::Wonders,
        GreedyKey::WonderProgress,
        GreedyKey::Hand,
        GreedyKey::TechLevels,
    ];

    /// The exact string Python's `WEIGHTS`/`features()` use for this key --
    /// I/O only (the differential test), nothing on the evaluation hot path
    /// needs it.
    pub const fn name(self) -> &'static str {
        match self {
            GreedyKey::Culture => "culture",
            GreedyKey::CultureRate => "culture_rate",
            GreedyKey::ScienceRate => "science_rate",
            GreedyKey::Science => "science",
            GreedyKey::Strength => "strength",
            GreedyKey::FoodRate => "food_rate",
            GreedyKey::ResourceRate => "resource_rate",
            GreedyKey::Food => "food",
            GreedyKey::Resources => "resources",
            GreedyKey::Workers => "workers",
            GreedyKey::FreeWorkers => "free_workers",
            GreedyKey::YellowBank => "yellow_bank",
            GreedyKey::CivilActions => "civil_actions",
            GreedyKey::MilitaryActions => "military_actions",
            GreedyKey::HappyMargin => "happy_margin",
            GreedyKey::Wonders => "wonders",
            GreedyKey::WonderProgress => "wonder_progress",
            GreedyKey::Hand => "hand",
            GreedyKey::TechLevels => "tech_levels",
        }
    }

    /// `WEIGHTS[self.name()]` -- FROZEN, see this module's top doc comment.
    pub const fn default_weight(self) -> f64 {
        match self {
            GreedyKey::Culture => 1.0,
            GreedyKey::CultureRate => 6.0,
            GreedyKey::ScienceRate => 4.0,
            GreedyKey::Science => 0.5,
            GreedyKey::Strength => 0.8,
            GreedyKey::FoodRate => 1.2,
            GreedyKey::ResourceRate => 1.5,
            GreedyKey::Food => 0.2,
            GreedyKey::Resources => 0.3,
            GreedyKey::Workers => 1.5,
            GreedyKey::FreeWorkers => 0.3,
            GreedyKey::YellowBank => 0.15,
            GreedyKey::CivilActions => 2.0,
            GreedyKey::MilitaryActions => 0.7,
            GreedyKey::HappyMargin => 1.5,
            GreedyKey::Wonders => 3.0,
            GreedyKey::WonderProgress => 1.0,
            GreedyKey::Hand => 0.4,
            GreedyKey::TechLevels => 1.5,
        }
    }

    /// Parse a printed name. **I/O only** -- mirrors
    /// `weighted::weights::WeightKey::by_name`'s identical linear scan and
    /// reasoning: a second, hashed index is surface nothing on the search hot
    /// path is allowed to need.
    pub fn by_name(name: &str) -> Option<GreedyKey> {
        GreedyKey::ALL.iter().copied().find(|k| k.name() == name)
    }
}

const N: usize = GreedyKey::ALL.len();

/// `WEIGHTS`, or a caller-supplied override of it -- Python's
/// `weights: dict[str, float] | None`. `[f64; N]` indexed by `key as usize`,
/// not a `HashMap<String, f64>`, for the same reason
/// `weighted::weights::Weights` gives: this is read once per candidate move
/// of every 1-ply search.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GreedyWeights([f64; N]);

impl GreedyWeights {
    pub fn defaults() -> GreedyWeights {
        let mut out = [0.0; N];
        for &k in GreedyKey::ALL {
            out[k as usize] = k.default_weight();
        }
        GreedyWeights(out)
    }

    #[inline]
    pub fn get(&self, key: GreedyKey) -> f64 {
        self.0[key as usize]
    }

    #[inline]
    pub fn set(&mut self, key: GreedyKey, value: f64) {
        self.0[key as usize] = value;
    }
}

impl Default for GreedyWeights {
    fn default() -> Self {
        GreedyWeights::defaults()
    }
}

/// The raw feature vector [`evaluate`] prices -- Python's `features()`
/// return value, as an array indexed by [`GreedyKey`] for the same reason
/// `weighted::features::Features` is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GreedyFeatures([f64; N]);

impl GreedyFeatures {
    #[inline]
    pub fn get(&self, key: GreedyKey) -> f64 {
        self.0[key as usize]
    }

    #[inline]
    fn set(&mut self, key: GreedyKey, value: f64) {
        self.0[key as usize] = value;
    }
}

impl Default for GreedyFeatures {
    fn default() -> Self {
        GreedyFeatures([0.0; N])
    }
}

/// `features(state, p)`: the 19 coordinates [`evaluate`] dots against
/// [`GreedyWeights`]. See this module's top doc comment for why `workers`
/// and (the pre-government half of) `tech_levels` are read off
/// [`sweep_tableau`] rather than re-walking `p.techs` here.
pub fn features(state: &GameState, idx: u8) -> GreedyFeatures {
    let p = &state.players[idx as usize];
    let s = effects::compute(state, p);
    let sweep = sweep_tableau(p);
    let tech_levels = sweep.tech_levels + i32::from(p.government.level());
    let happy_margin = f64::from(s.happy) - f64::from(economy::happy_required(p.yellow_bank));

    // Wonder progress: stages already paid for, mirroring the identical
    // slice-and-sum `weighted::features::features` does for its own
    // `wonder_progress` coordinate (this module has no `remaining`/
    // `turns_to_finish`/`overrun` counterpart -- GreedyBot never priced
    // those).
    let mut progress = 0i32;
    if !p.wonder.is_none() {
        let stages = p.wonder.get().stages;
        let built = (p.wonder_steps as usize).min(stages.len());
        progress = stages[..built].iter().map(|&st| i32::from(st)).sum();
    }

    let mut f = GreedyFeatures::default();
    f.set(GreedyKey::Culture, f64::from(p.culture));
    f.set(GreedyKey::CultureRate, f64::from(s.culture));
    f.set(GreedyKey::ScienceRate, f64::from(s.science));
    f.set(GreedyKey::Science, f64::from(p.science));
    f.set(GreedyKey::Strength, f64::from(s.strength));
    f.set(GreedyKey::FoodRate, f64::from(s.food));
    f.set(GreedyKey::ResourceRate, f64::from(s.resources));
    f.set(GreedyKey::Food, f64::from(p.food));
    f.set(GreedyKey::Resources, f64::from(p.resources));
    f.set(GreedyKey::Workers, f64::from(sweep.workers));
    f.set(GreedyKey::FreeWorkers, f64::from(p.workers_free));
    f.set(GreedyKey::YellowBank, f64::from(p.yellow_bank));
    f.set(GreedyKey::CivilActions, f64::from(s.civil_actions));
    f.set(GreedyKey::MilitaryActions, f64::from(s.military_actions));
    // `min(2, happy_margin)` -- Python's `min(2, happy_margin)` with a
    // Python `int` `2` against a `float` `happy_margin`; the clamp itself is
    // the only thing that matters here.
    f.set(GreedyKey::HappyMargin, happy_margin.min(2.0));
    f.set(GreedyKey::Wonders, p.completed_wonders.len() as f64);
    f.set(GreedyKey::WonderProgress, f64::from(progress));
    f.set(GreedyKey::Hand, p.hand_civil.len() as f64);
    f.set(GreedyKey::TechLevels, f64::from(tech_levels));
    f
}

/// `evaluate(state, idx, weights=None)`: the weighted sum of [`features`]
/// minus `0.4 * best_rival_culture` -- being ahead of the best rival is what
/// wins the game. See this module's top doc comment for why this sums plain
/// `f64` rather than porting `math.fsum`.
pub fn evaluate(state: &GameState, idx: u8, w: &GreedyWeights) -> f64 {
    let f = features(state, idx);
    let mut own = 0.0;
    for &k in GreedyKey::ALL {
        own += w.get(k) * f.get(k);
    }

    let mut best_rival = 0u16;
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx != idx && !q.resigned {
            best_rival = best_rival.max(q.culture);
        }
    }
    own - 0.4 * f64::from(best_rival)
}

// ------------------------------------------------------------------- bot

/// 1-ply lookahead under the frozen [`GreedyWeights`] evaluator -- the
/// fingerprint CONTROL. See this module's top doc comment for the Python
/// mechanisms this struct does not carry and why each omission is safe.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GreedyBot {
    pub weights: GreedyWeights,
}

impl Default for GreedyBot {
    fn default() -> Self {
        GreedyBot { weights: GreedyWeights::default() }
    }
}

impl GreedyBot {
    pub fn new(weights: GreedyWeights) -> GreedyBot {
        GreedyBot { weights }
    }

    /// Best move for `state.decider()` among `moves`: apply each candidate to
    /// a clone, score it with [`evaluate`], and take the argmax (first
    /// candidate wins a tie). `Move::Resign` is filtered out unconditionally
    /// first -- see this module's top doc comment, point 4, for why that is
    /// not a configurable field.
    ///
    /// `end_turn` is penalised `-0.01`: "ending the turn is never rewarded
    /// for its own sake; it only wins when nothing else improves the
    /// position" (`__init__.py`'s own comment on the identical line).
    ///
    /// # Panics
    /// If `moves` is empty (a caller bug, matching every other bot in this
    /// port).
    pub fn choose(&self, state: &GameState, moves: &[Move]) -> Move {
        let filtered = super::filter_resign(moves, false);
        let moves: &[Move] = filtered.as_slice();
        if moves.len() == 1 {
            return moves[0];
        }

        let idx = state.decider();
        let w = &self.weights;
        let mut best: Option<(Move, f64)> = None;
        for &mv in moves {
            let mut trial = state.clone();
            apply::apply(&mut trial, mv);
            let mut val = evaluate(&trial, idx, w);
            if matches!(mv, Move::EndTurn) {
                val -= 0.01;
            }
            if best.is_none_or(|(_, bv)| val > bv) {
                best = Some((mv, val));
            }
        }
        best.map(|(m, _)| m).unwrap_or(moves[0])
    }

    /// `__call__`: generates `moves` itself, matching [`RandomBot::pick`].
    pub fn pick(&self, state: &GameState) -> Move {
        let moves = legal::legal_moves(state);
        self.choose(state, moves.as_slice())
    }
}

// -------------------------------------------------------------- make_bots

/// Which bot [`make_bots`]/[`BotKind::from_str`] builds -- Python's
/// `make_bots` kind strings. See this module's top doc comment for the one
/// deliberate difference from Python (`from_str` rejects an unrecognized
/// name instead of silently building a [`GreedyBot`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum BotKind {
    Random,
    Greedy,
    Weighted,
    Quiescent,
    Plan,
}

impl BotKind {
    pub const ALL: &'static [BotKind] =
        &[BotKind::Random, BotKind::Greedy, BotKind::Weighted, BotKind::Quiescent, BotKind::Plan];

    pub const fn name(self) -> &'static str {
        match self {
            BotKind::Random => "random",
            BotKind::Greedy => "greedy",
            BotKind::Weighted => "weighted",
            BotKind::Quiescent => "quiescent",
            BotKind::Plan => "plan",
        }
    }
}

impl FromStr for BotKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        BotKind::ALL.iter().copied().find(|k| k.name() == s).ok_or_else(|| {
            format!(
                "unknown bot kind {s:?}, want one of {}",
                BotKind::ALL.iter().map(|k| k.name()).collect::<Vec<_>>().join(", ")
            )
        })
    }
}

/// A constructed bot, ready to decide via [`Bot::pick`] -- one per
/// [`BotKind`] variant, each carrying whatever its own module's
/// `choose`/`pick` needs (an RNG stream, a weight vector, search
/// configuration and counters). Exists so [`make_bots`] can return one `Vec`
/// mixing kinds, exactly as Python's `spec` (e.g. `"greedy,random"`)
/// requires; a caller who only ever wants one concrete kind should reach for
/// that module directly instead; this enum adds a `match` on top for exactly
/// the cases `make_bots` needs to mix.
pub enum Bot {
    Random(RandomBot),
    Greedy(GreedyBot),
    Weighted(weighted::eval::WeightedBot),
    Quiescent { cfg: quiescent::QuiescenceConfig, weights: weighted::weights::Weights, stats: quiescent::Stats },
    Plan { cfg: plan::PlanConfig, stats: plan::Stats, counters: pending::Counters, rng: PyRandom },
}

impl Bot {
    /// `__call__`: generates `moves` itself via [`legal::legal_moves`] and
    /// dispatches to the owning module's search. The quiescent branch builds
    /// its `eval` closure from `weights` on the spot rather than storing one
    /// -- `quiescent::pick`'s own signature is generic over the scorer
    /// precisely so a caller can do this with no allocation and no trait
    /// object.
    /// Which kind this is. `make_bots` cycles a spec over the seats, so a
    /// caller that wants to tally results per bot has to be able to ask a
    /// seat what it is rather than re-deriving it from the spec string and
    /// the seat index -- re-deriving it is how a tally ends up labelling the
    /// wrong bot the moment rotation or an odd seat count is introduced.
    pub const fn kind(&self) -> BotKind {
        match self {
            Bot::Random(_) => BotKind::Random,
            Bot::Greedy(_) => BotKind::Greedy,
            Bot::Weighted(_) => BotKind::Weighted,
            Bot::Quiescent { .. } => BotKind::Quiescent,
            Bot::Plan { .. } => BotKind::Plan,
        }
    }

    pub fn pick(&mut self, state: &GameState) -> Move {
        let moves = legal::legal_moves(state);
        let moves = moves.as_slice();
        match self {
            Bot::Random(bot) => bot.choose(moves),
            Bot::Greedy(bot) => bot.choose(state, moves),
            Bot::Weighted(bot) => bot.choose(state, moves),
            Bot::Quiescent { cfg, weights, stats } => {
                let w = *weights;
                let eval = move |s: &GameState, i: u8| weighted::eval::evaluate(s, i, &w, None, None);
                quiescent::pick(cfg, stats, state, moves, &eval)
            }
            Bot::Plan { cfg, stats, counters, rng } => plan::pick(cfg, stats, counters, rng, state, moves),
        }
    }
}

/// `make_bots(spec, num_players, seed=0)`: build one [`Bot`] per player,
/// cycling `spec`'s comma-separated kinds round-robin. See this module's top
/// doc comment for the seeding shape and the `BotKind::Plan` width override.
///
/// # Errors
/// If `spec` names an unrecognized kind -- see this module's top doc comment
/// for why that is a hard error here rather than Python's silent
/// `GreedyBot` fallback.
pub fn make_bots(spec: &str, num_players: u8, seed: i64) -> Result<Vec<Bot>, String> {
    let seats = make_seats(spec, num_players, weighted::weights::Weights::default())?;
    Ok(build_bots(&seats, seed))
}

/// One seat's assignment: which kind sits there, and which weight vector its
/// evaluator reads.
///
/// The kind and its weights travel together in one struct rather than in two
/// lists indexed by seat, because the arena's whole job is to sit two
/// DIFFERENT vectors of the SAME kind at one table -- and two parallel lists
/// is exactly the shape that lets a result table report the challenger's
/// score under the champion's name. See [`Bot::kind`] for the same argument
/// applied to reading a seat back out.
#[derive(Clone, Debug)]
pub struct Seat {
    pub kind: BotKind,
    /// Ignored by [`BotKind::Random`] and [`BotKind::Greedy`], which have no
    /// linear evaluator to read it -- `GreedyBot` scores with its own
    /// separate feature set, and `RandomBot` scores nothing at all.
    pub weights: weighted::weights::Weights,
}

/// Parse a spec into one [`Seat`] per player, all sharing `weights`.
///
/// This is where the comma-separated spec is cycled round-robin over the
/// seats; it stays the ONLY place that parses one.
///
/// # Errors
/// If `spec` names an unrecognized kind -- see this module's top doc comment
/// for why that is a hard error here rather than Python's silent
/// `GreedyBot` fallback.
pub fn make_seats(
    spec: &str,
    num_players: u8,
    weights: weighted::weights::Weights,
) -> Result<Vec<Seat>, String> {
    let mut kinds = Vec::new();
    for part in spec.split(',') {
        kinds.push(BotKind::from_str(part)?);
    }
    if kinds.is_empty() {
        return Err("make_bots: spec has no kinds".to_string());
    }
    Ok((0..num_players)
        .map(|i| Seat { kind: kinds[(i as usize) % kinds.len()], weights })
        .collect())
}

/// Build the bots for an already-decided table.
///
/// Seeding is per seat (`seed * 131 + i`, as Python's `make_bots` does), so
/// two seats of the same kind at one table do not draw the same numbers.
pub fn build_bots(seats: &[Seat], seed: i64) -> Vec<Bot> {
    let mut out = Vec::with_capacity(seats.len());
    for (i, seat) in seats.iter().enumerate() {
        let player_seed = seed.wrapping_mul(131).wrapping_add(i as i64);
        out.push(match seat.kind {
            BotKind::Random => Bot::Random(RandomBot::new(player_seed as u64)),
            BotKind::Greedy => Bot::Greedy(GreedyBot::default()),
            BotKind::Weighted => {
                Bot::Weighted(weighted::eval::WeightedBot::new(seat.weights))
            }
            BotKind::Quiescent => Bot::Quiescent {
                cfg: quiescent::QuiescenceConfig::default(),
                weights: seat.weights,
                stats: quiescent::Stats::default(),
            },
            BotKind::Plan => Bot::Plan {
                cfg: plan::PlanConfig {
                    width: 2,
                    weights: seat.weights,
                    ..plan::PlanConfig::default()
                },
                stats: plan::Stats::default(),
                counters: pending::Counters::default(),
                rng: PyRandom::new(player_seed),
            },
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;

    // ------------------------------------------------------------ features

    #[test]
    fn features_actually_writes_something_beyond_the_zero_default() {
        let state = G::new_game(3, 43);
        let f = features(&state, 0);
        assert_ne!(f, GreedyFeatures::default(), "features() must not return an all-zero vector on a real deal");
    }

    #[test]
    fn every_greedy_key_name_round_trips() {
        for &k in GreedyKey::ALL {
            assert_eq!(GreedyKey::by_name(k.name()), Some(k), "{}", k.name());
        }
    }

    #[test]
    fn wonder_progress_is_zero_with_no_wonder_in_progress() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 40);
            for idx in 0..n {
                let f = features(&state, idx);
                assert_eq!(f.get(GreedyKey::WonderProgress), 0.0, "{n}p idx {idx}");
            }
        }
    }

    #[test]
    fn happy_margin_is_never_above_two() {
        let state = G::new_game(2, 41);
        let f = features(&state, 0);
        assert!(f.get(GreedyKey::HappyMargin) <= 2.0);
    }

    // ------------------------------------------------------------ evaluate

    #[test]
    fn evaluate_on_a_fresh_deal_is_finite() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 100);
            let w = GreedyWeights::default();
            for idx in 0..n {
                let v = evaluate(&state, idx, &w);
                assert!(v.is_finite(), "{n}p idx {idx}: {v}");
            }
        }
    }

    #[test]
    fn a_bigger_rival_culture_lowers_the_score() {
        let mut state = G::new_game(2, 101);
        let w = GreedyWeights::default();
        let before = evaluate(&state, 0, &w);
        state.players[1].culture += 10;
        let after = evaluate(&state, 0, &w);
        assert!(after < before, "a bigger rival culture must lower idx 0's score");
    }

    // ------------------------------------------------------------ GreedyBot

    #[test]
    fn choose_with_a_single_move_returns_it_directly() {
        let state = G::new_game(2, 1);
        let moves = legal::legal_moves(&state);
        let one = [moves.as_slice()[0]];
        let bot = GreedyBot::default();
        assert_eq!(bot.choose(&state, &one), one[0]);
    }

    #[test]
    fn choose_never_mutates_the_real_state() {
        let state = G::new_game(2, 2);
        let before = state.clone();
        let moves = legal::legal_moves(&state);
        let bot = GreedyBot::default();
        let _ = bot.choose(&state, moves.as_slice());
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.turn, before.turn);
    }

    #[test]
    fn choose_always_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 3);
            let moves = legal::legal_moves(&state);
            let bot = GreedyBot::default();
            let mv = bot.choose(&state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    #[test]
    fn resign_is_filtered_out_when_a_live_alternative_exists() {
        let state = G::new_game(2, 4);
        let bot = GreedyBot::default();
        let picked = bot.choose(&state, &[Move::Resign, Move::EndTurn]);
        assert_eq!(picked, Move::EndTurn, "resign must never be chosen over a live alternative");
    }

    #[test]
    fn a_lone_resign_candidate_is_still_returned() {
        // Resign filtering falls back to the ORIGINAL move list when
        // filtering it would leave nothing -- mirrors Python's
        // `live = [...] or moves`.
        let state = G::new_game(2, 5);
        let bot = GreedyBot::default();
        assert_eq!(bot.choose(&state, &[Move::Resign]), Move::Resign);
    }

    // ------------------------------------------------------------ RandomBot

    #[test]
    fn random_bot_always_returns_an_offered_move() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 6);
            let moves = legal::legal_moves(&state);
            let mut bot = RandomBot::new(7);
            let mv = bot.choose(moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{n}p: {mv:?} was not offered");
        }
    }

    #[test]
    fn random_bot_filters_resign_by_default() {
        let mut bot = RandomBot::new(1);
        for _ in 0..50 {
            assert_eq!(bot.choose(&[Move::Resign, Move::EndTurn]), Move::EndTurn);
        }
    }

    #[test]
    fn random_bot_with_allow_resign_can_return_a_lone_resign() {
        let mut bot = RandomBot::new(1);
        bot.allow_resign = true;
        assert_eq!(bot.choose(&[Move::Resign]), Move::Resign);
    }

    // ---------------------------------------------------------- BotKind

    #[test]
    fn every_bot_kind_name_round_trips() {
        for &k in BotKind::ALL {
            assert_eq!(BotKind::from_str(k.name()), Ok(k), "{}", k.name());
        }
    }

    #[test]
    fn an_unknown_kind_is_a_parse_error_not_a_silent_greedy_bot() {
        assert!(BotKind::from_str("not_a_real_kind").is_err());
    }

    // ---------------------------------------------------------- make_bots

    #[test]
    fn make_bots_cycles_kinds_round_robin() {
        let bots = make_bots("greedy,random", 4, 0).unwrap();
        assert_eq!(bots.len(), 4);
        assert!(matches!(bots[0], Bot::Greedy(_)));
        assert!(matches!(bots[1], Bot::Random(_)));
        assert!(matches!(bots[2], Bot::Greedy(_)));
        assert!(matches!(bots[3], Bot::Random(_)));
    }

    #[test]
    fn make_bots_rejects_an_unknown_kind() {
        assert!(make_bots("greedy,not_a_kind", 2, 0).is_err());
    }

    #[test]
    fn make_bots_plan_uses_width_two() {
        let bots = make_bots("plan", 1, 0).unwrap();
        match &bots[0] {
            Bot::Plan { cfg, .. } => assert_eq!(cfg.width, 2),
            other => panic!("expected Bot::Plan, got a different kind: {}", matches_name(other)),
        }
    }

    fn matches_name(b: &Bot) -> &'static str {
        match b {
            Bot::Random(_) => "random",
            Bot::Greedy(_) => "greedy",
            Bot::Weighted(_) => "weighted",
            Bot::Quiescent { .. } => "quiescent",
            Bot::Plan { .. } => "plan",
        }
    }

    #[test]
    fn every_bot_kind_picks_a_legal_move() {
        for &k in BotKind::ALL {
            let mut bots = make_bots(k.name(), 2, 99).unwrap();
            let state = G::new_game(2, 9);
            let moves = legal::legal_moves(&state);
            let mv = bots[0].pick(&state);
            assert!(moves.as_slice().contains(&mv), "{}: {mv:?} was not offered", k.name());
        }
    }
}
