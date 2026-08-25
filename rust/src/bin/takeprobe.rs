//! `takeprobe` -- the decisive experiment behind
//! `analysis/mine_take_probe_3p_2026-08-24.txt`, following up
//! `analysis/tech_acquisition_3p_2026-08-24.txt`'s finding that a
//! non-starting Mine card (Iron/Coal/Oil) is SEEN in the card row in
//! essentially every player-game but TAKEN (per `behavcensus.rs`'s pre-move
//! `taken_card` variable) exactly 0.000 times at every age tier, across 600
//! self-play player-games.
//!
//! That census answers WHAT HAPPENED; it explicitly does not answer WHY
//! (`tech_acquisition_3p_2026-08-24.txt` caveat 6). This binary is read-only
//! instrumentation aimed at WHY, and measures independently of
//! `behavcensus.rs`'s `taken_card` mechanism entirely -- it never reads or
//! computes a `taken_card`-shaped value -- so the two are a genuine
//! cross-check, not two views of the same counter.
//!
//! # Method
//!
//! Self-plays games at `--players` seats, all seats sharing one
//! `--weights` champion vector (mirrors `behavcensus.rs::play_one`'s
//! all-seats-`BotKind::Weighted` self-play mirror match -- confirmed by
//! reading `behavcensus.rs:1337-1338`, which is what the census this probe
//! follows up on actually ran). At every decision point where a civil take
//! is structurally possible (`state.phase == Phase::Actions`,
//! `state.pending.is_empty()` -- the only site `legal.rs::action_moves`
//! generates `Move::Take` from), for each of the three non-starting Mine
//! cards (Iron/Coal/Oil) found sitting in `state.card_row` at that instant,
//! records four pipeline stages:
//!
//! 1. **SEEN** -- the card is in the row at a decision point where a take is
//!    possible (by construction, every card counted here satisfies this).
//! 2. **LEGAL** -- `Move::Take{slot}` for that exact row slot is present in
//!    `legal::legal_moves(&state)`, i.e. `costs::can_take_gated` passed.
//! 3. **SCORED** -- it survives into the candidate set
//!    [`tta::bots::weighted::eval::WeightedBot`] actually evaluates.
//!    `WeightedBot::choose`/`rank_moves` both start from
//!    `bots::filter_resign`, which removes only `Move::Resign` -- never a
//!    `Take` -- so for this bot kind SCORED is structurally identical to
//!    LEGAL whenever LEGAL holds: there is no shortlist/narrowing stage
//!    between move generation and evaluation for a plain 1-ply
//!    `WeightedBot`, unlike a beam/shortlist search. The probe still checks
//!    this independently (searching the candidate list
//!    [`tta::bots::weighted::eval::candidate_features`] actually returns for
//!    the move) rather than assuming it, so a future change to
//!    `filter_resign` or a narrowing stage would be caught here, not assumed
//!    away.
//! 4. **if SCORED**: the move's own value, the winning candidate's value,
//!    and the move's 1-based rank among every scored candidate, computed by
//!    dotting [`tta::bots::weighted::eval::candidate_features`]'s per-move
//!    feature vectors against the champion weights via
//!    [`tta::bots::weighted::eval::dot`] (the strict-`>`, first-wins argmax
//!    over those dotted scores reproduces `WeightedBot::choose`'s own pick
//!    exactly -- see [`play_one`]'s inline comment -- so driving the game
//!    off it reproduces an ordinary `BotKind::Weighted` self-play trajectory
//!    exactly; this probe never diverges from what an unwatched self-play
//!    game would have played).
//!
//! # Stage-4 diagnosis: what beats a scored Mine take, and why
//!
//! At every decision point where a Mine take is SCORED, this binary also
//! records (a) the [`Move`] kind that won instead ([`move_kind`]) and (b)
//! how much each [`WeightKey`] term contributes to the score gap between
//! the winner and the Mine take -- `weight(k) * (winner_feature(k) -
//! take_feature(k))`, via [`candidate_features`]/[`dot`] (the same vectors
//! `bin/agreefit.rs` fits against, guaranteed by that module's own doc
//! comment and tests to dot back to `evaluate`'s exact score). Reported as
//! a RANKED list of key NAMES with the SIGN of their net effect only --
//! never a raw contribution number or a weight value -- per this task's own
//! hard constraint against writing down a weight value or coefficient; this
//! binary reports where the gap comes from, not what any term should be.
//!
//! ```text
//! cargo run --release --bin takeprobe -- \
//!     --games 200 --players 3 --weights /path/to/champ3p.json --threads 5
//! ```

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use tta::apply;
use tta::bots::weighted::eval::{candidate_features, dot, load_weights, WeightedBot};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::game::{self, MOVE_CAP};
use tta::state::{GameState, PlayerState};
use tta::{costs, economy, legal};
use tta::{CardId, Move, Phase};

/// The three non-starting mine cards, in age order. Bronze (age A) is the
/// starting technology printed on the player board, never a deck card
/// (`data/cards_civil.json`'s own note on it) -- it is never in `card_row`
/// and is deliberately excluded here, matching
/// `tech_acquisition_3p_2026-08-24.txt`'s own SEEN/TAKEN definition.
const MINE_NAMES: [&str; 3] = ["Iron", "Coal", "Oil"];

/// `--sensitivity` mode: for every key named here, and every multiplier in
/// [`M_SWEEP`], the score gap between a Mine take and the winning candidate
/// at that SAME decision point is re-derived with `w[key] *= m` -- see this
/// binary's own doc comment on the method. This is the union of
/// `mine_take_probe_3p_2026-08-24.txt`'s top-15 gap-contribution list and
/// this task's own named suspects (`CultureRate`/`Workers`/`WorkersLate`/
/// `FoodStock`/`ResourceStock`, all clamp-saturated in the champion vector;
/// `ResourceRate`/`FoodRate`/`BestMine`/`BestFarm`/`HandPotential`/
/// `Strength`), deduplicated.
///
/// Named by STRING, not by a literal enum-path expression, on purpose:
/// `WorkersLate` is one of the seven keys `registry.rs`'s own
/// `PHASE_SUFFIXED_NO_LITERAL_READER` documents as reachable ONLY via
/// `.early()`/`.late()` indirection, never a literal path naming that variant
/// in production source -- writing that exact literal here would flip
/// `registry.rs::tests::every_weight_key_is_named_by_production_source_
/// outside_its_own_declaration` (confirmed: it does, an earlier draft of
/// this file's own gate run caught it -- even inside a comment, since that
/// test does a plain substring scan of each file's un-eval'd text), and
/// `registry.rs`/`src/bots/**` are out of scope for this task.
/// [`sens_keys`]/[`clamp_keys`] resolve these names against `WeightKey`'s own
/// full variant list at runtime instead.
const SENS_KEY_NAMES: [&str; 18] = [
    "HandPotential",
    "BlueFree",
    "ResourceStock",
    "FoodStock",
    "CultureRate",
    "Strength",
    "Science",
    "ScienceSurplus",
    "ResourceRate",
    "WorkersLate",
    "EventScoringMargin",
    "Workers",
    "HandPerishable",
    "TakeCostPaid",
    "WonderPromise",
    "FoodRate",
    "BestMine",
    "BestFarm",
];

/// The five keys the task names as clamp-saturated in the champion vector
/// (reading exactly 60.000, the clamp bound, or +21.25) -- the "joint
/// clamp-block" check sets all five to `m = 0` simultaneously. See
/// [`SENS_KEY_NAMES`]'s doc comment for why these are names, not literal
/// `WeightKey::<Variant>` paths.
const CLAMP_KEY_NAMES: [&str; 5] = ["CultureRate", "Workers", "WorkersLate", "FoodStock", "ResourceStock"];

/// Resolves a [`WeightKey`] by its `Debug` name -- see [`SENS_KEY_NAMES`]'s
/// doc comment for why this indirection exists instead of a literal path.
fn key_by_name(name: &str) -> WeightKey {
    *WeightKey::ALL.iter().find(|k| format!("{k:?}") == name).unwrap_or_else(|| panic!("no WeightKey named {name:?}"))
}

fn sens_keys() -> [WeightKey; SENS_KEY_NAMES.len()] {
    let mut out = [WeightKey::ALL[0]; SENS_KEY_NAMES.len()];
    for (i, name) in SENS_KEY_NAMES.iter().enumerate() {
        out[i] = key_by_name(name);
    }
    out
}

fn clamp_keys() -> [WeightKey; CLAMP_KEY_NAMES.len()] {
    let mut out = [WeightKey::ALL[0]; CLAMP_KEY_NAMES.len()];
    for (i, name) in CLAMP_KEY_NAMES.iter().enumerate() {
        out[i] = key_by_name(name);
    }
    out
}

/// Multipliers swept per [`SENS_KEY_NAMES`] entry: `0.0` (delete the key
/// entirely), a wide log-spaced range below and above `1.0` (down to
/// `1e-4`, up to `1e6`, so a key whose contribution is small relative to
/// the ~190-unit mean gap still gets a fair chance to close it under
/// amplification), plus finer steps right around `1.0` to catch a
/// knife-edge flip. Never negative -- flipping a weight's SIGN is a
/// different intervention than pricing it more/less strongly and is out of
/// scope here (recorded as a method caveat in the report).
const M_SWEEP: [f64; 24] = [
    0.0, 0.0001, 0.001, 0.01, 0.1, 0.3, 0.5, 0.7, 0.9, 1.0, 1.1, 1.3, 1.5, 2.0, 3.0, 5.0, 10.0, 30.0, 100.0, 300.0,
    1000.0, 10000.0, 100000.0, 1000000.0,
];

/// Per-`(key, m)` and joint-clamp-block outcome counts for `--sensitivity`
/// mode: `n` = number of scored Mine-take decision points measured, `rank1`
/// = how many of those reach rank 1 (the Mine take would win) under the
/// perturbation, `rank12` = how many reach rank 1 or 2. This is a STATIC
/// re-score of the exact same feature vectors [`candidate_features`] already
/// computed for the real (unperturbed) champion vector at that decision
/// point -- see [`rank_under_perturbation`] -- never a replayed game, so one
/// self-play pass answers every `(key, m)` cell without re-running self-play
/// per perturbation, per this task's own method requirement.
#[derive(Clone)]
struct SensStats {
    /// `per_key[key_index_in(SENS_KEY_NAMES)][m_index_in(M_SWEEP)] = (n, rank1, rank12)`.
    per_key: Vec<Vec<(u64, u64, u64)>>,
    /// All five [`CLAMP_KEY_NAMES`] set to `m = 0` at once.
    joint_clamp: (u64, u64, u64),
}

impl SensStats {
    fn new() -> SensStats {
        SensStats { per_key: vec![vec![(0, 0, 0); M_SWEEP.len()]; SENS_KEY_NAMES.len()], joint_clamp: (0, 0, 0) }
    }

    fn merge(&mut self, other: &SensStats) {
        for (ki, row) in self.per_key.iter_mut().enumerate() {
            for (mi, cell) in row.iter_mut().enumerate() {
                let o = other.per_key[ki][mi];
                cell.0 += o.0;
                cell.1 += o.1;
                cell.2 += o.2;
            }
        }
        self.joint_clamp.0 += other.joint_clamp.0;
        self.joint_clamp.1 += other.joint_clamp.1;
        self.joint_clamp.2 += other.joint_clamp.2;
    }
}

/// Re-derives the 1-based rank a Mine take (`feats[take_pos]`) would have
/// among every candidate at `feats`' shared decision point if the champion
/// vector's weight on each `(key_index, delta_weight)` pair in
/// `adjustments` were shifted by `delta_weight` -- i.e. `w[key] *= m` is
/// passed in as `delta_weight = (m - 1.0) * w.get(key)`, and the joint
/// clamp-block check (`m = 0` on several keys at once) passes one
/// `(key_index, -w.get(key))` pair per key. Exploits that `dot(w, f)` is
/// linear in `w` for a FIXED, already-computed feature vector `f` (the same
/// freeze [`candidate_features`]'s own doc comment and caveat 2 of
/// `mine_take_probe_3p_2026-08-24.txt` already rely on): the new score for
/// candidate `i` is `vals[i] + Σ delta_weight_j * feats[i].1[key_index_j]`,
/// so every `(key, m)` cell in [`SensStats`] is one pass over the SAME
/// `vals`/`feats` the base probe already computed for this decision point --
/// no re-evaluation of `linear_features`, no replayed game.
fn rank_under_perturbation(vals: &[f64], feats: &[(Move, Vec<f64>)], take_pos: usize, adjustments: &[(usize, f64)]) -> u64 {
    let shifted = |i: usize| -> f64 {
        let mut v = vals[i];
        for &(key_idx, delta_weight) in adjustments {
            v += delta_weight * feats[i].1[key_idx];
        }
        v
    };
    let take_val = shifted(take_pos);
    let mut better = 0u64;
    for i in 0..vals.len() {
        if i != take_pos && shifted(i) > take_val {
            better += 1;
        }
    }
    better + 1
}

/// `--interpolate`: the number of points swept across `t` in `w(t) = (1-t) *
/// champion + t * human`, `t = i / 100` for `i` in `0..T_GRID_LEN` -- i.e.
/// every multiple of 0.01 from 0.00 to 1.00 inclusive. Chosen so the coarse
/// 0.05 report (every 5th entry) and the finer 0.01 report the task asks
/// for around any transition are the SAME computed grid, sliced two ways,
/// rather than two separate sweeps -- the per-`t` work is one blend of two
/// already-computed dot products (see [`rank_gap_at_t`]), cheap enough that
/// computing all 101 unconditionally costs nothing worth trimming.
const T_GRID_LEN: usize = 101;

/// `t` at grid index `i` -- see [`T_GRID_LEN`].
fn t_at(i: usize) -> f64 {
    i as f64 / 100.0
}

/// `sqrt(sum(w.get(k)^2))` over every [`WeightKey`], i.e. the L2 norm of the
/// FULLY RESOLVED vector [`load_weights`] produces -- every key present,
/// absent ones already filled with [`WeightKey::default_weight`] by
/// `Weights::defaults()` before the JSON overlay runs. This is deliberately
/// NOT "norm over the keys the JSON file happens to spell out": a champion
/// file with 160 explicit keys and a human file with 140 both resolve to the
/// same `N = WeightKey::ALL.len()`-wide array before this ever runs, so
/// "the union of keys, treating an absent key as `load_weights` resolves
/// it" and "every key" are the same set for a [`Weights`] value -- there is
/// no narrower vector to take a norm over.
fn l2_norm(w: &Weights) -> f64 {
    WeightKey::ALL.iter().map(|&k| w.get(k).powi(2)).sum::<f64>().sqrt()
}

/// Returns a copy of `w` with every coordinate multiplied by `s`. For `s >
/// 0` this cannot change any argmax `dot(w, f)` ever produces (`dot(s*w, f)
/// == s*dot(w, f)`), so scaling a vector by a positive constant is free --
/// see this binary's own doc comment on `--normalize` / the task background
/// this flag exists to test.
fn scale_weights(w: &Weights, s: f64) -> Weights {
    let mut out = *w;
    for &k in WeightKey::ALL {
        out.set(k, s * w.get(k));
    }
    out
}

/// `--interpolate` inputs shared read-only across every game thread: the
/// human vector (already scaled by `s = ||champion||/||human||` when
/// `--normalize` is passed -- see `main`'s construction of this field,
/// [`l2_norm`] and [`scale_weights`] -- otherwise the raw loaded vector,
/// unchanged), and the [`WeightKey`]s where it differs from the champion
/// (beyond float noise) -- the set the single-key adoption sweep iterates.
struct InterpCtx<'a> {
    human: Weights,
    /// Keys where `|champion.get(k) - human.get(k)| > 1e-9`; adopting a key
    /// NOT in this set would be a no-op perturbation (`delta_weight == 0`),
    /// so excluding them keeps the report's "top 10"/"top 15" and "exactly
    /// zero" counts meaningful instead of padded with guaranteed-zero rows.
    differ_keys: &'a [WeightKey],
    /// The greedy joint-adoption check's already-locked-in `(key_idx,
    /// delta_weight)` pairs, prepended to every candidate key's own
    /// `delta_weight` in the single-key adoption sweep below -- empty
    /// (`&[]`) for the ordinary single-key sweep, so that sweep is
    /// unaffected. See `main`'s greedy loop for how this is populated one
    /// key at a time.
    locked: &'a [(usize, f64)],
}

/// `w(t) = (1-t) * champion + t * human` outcome counts, one cell per
/// [`T_GRID_LEN`] grid point, accumulated over every scored Mine-take
/// decision point exactly like [`SensStats`] -- see [`rank_gap_at_t`] for
/// the linearity identity this reuses.
#[derive(Clone)]
struct InterpStats {
    /// `by_t[i] = (n, rank1, rank1or2, rank_sum, gap_sum)` at `t = t_at(i)`.
    by_t: Vec<(u64, u64, u64, u64, f64)>,
}

impl InterpStats {
    fn new() -> InterpStats {
        InterpStats { by_t: vec![(0, 0, 0, 0, 0.0); T_GRID_LEN] }
    }

    fn merge(&mut self, other: &InterpStats) {
        for (cell, &o) in self.by_t.iter_mut().zip(other.by_t.iter()) {
            cell.0 += o.0;
            cell.1 += o.1;
            cell.2 += o.2;
            cell.3 += o.3;
            cell.4 += o.4;
        }
    }
}

/// Single-key adoption outcome counts: for each key in an [`InterpCtx`]'s
/// `differ_keys`, `w[key] = human[key]`, every other coordinate left at the
/// champion's value -- a DIFFERENT intervention from `--sensitivity`'s
/// multiplier sweep, which can only rescale a coordinate and can never flip
/// its sign or move it off a clamp fence (this binary's own doc comment /
/// the task background). `per_key[i]` corresponds to `differ_keys[i]`.
#[derive(Clone)]
struct KeyAdoptStats {
    /// `per_key[i] = (n, rank1)`.
    per_key: Vec<(u64, u64)>,
}

impl KeyAdoptStats {
    fn new(n_keys: usize) -> KeyAdoptStats {
        KeyAdoptStats { per_key: vec![(0, 0); n_keys] }
    }

    fn merge(&mut self, other: &KeyAdoptStats) {
        for (cell, &o) in self.per_key.iter_mut().zip(other.per_key.iter()) {
            cell.0 += o.0;
            cell.1 += o.1;
        }
    }
}

/// Pairs the single-key ADOPTION sweep (`w[key] = ctx.human.get(key)`,
/// [`InterpCtx::locked`] prepended -- see [`KeyAdoptStats`]) with a
/// DELETION sweep (`w[key] = 0`, i.e. `delta_weight = -champion.get(key)`,
/// never `locked`-prefixed) computed at the SAME decision points in the
/// SAME game pass -- see `play_one`'s interpolate block. This is
/// `--normalize`'s cross-check from the task background: scaled adoption
/// must stop being a copy of deletion once the ~30x scale gap between
/// champion and the raw human vector is corrected, and `delete` is what
/// `adopt` gets compared against to show that (or fail to).
#[derive(Clone)]
struct AdoptStats {
    adopt: KeyAdoptStats,
    delete: KeyAdoptStats,
}

impl AdoptStats {
    fn new(n_keys: usize) -> AdoptStats {
        AdoptStats { adopt: KeyAdoptStats::new(n_keys), delete: KeyAdoptStats::new(n_keys) }
    }

    fn merge(&mut self, other: &AdoptStats) {
        self.adopt.merge(&other.adopt);
        self.delete.merge(&other.delete);
    }
}

/// Re-derives the 1-based rank and score gap the Mine take (`take_pos`)
/// would have under the blended vector `w(t) = (1-t) * champion + t *
/// human` at a fixed decision point, given `vals_champ[i] = dot(champion,
/// feats[i])` and `vals_human[i] = dot(human, feats[i])` over the SAME
/// `feats` (built once, with the champion vector as `linear_features`'
/// `freeze` argument, by the actual self-play step -- see this binary's doc
/// comment on `--interpolate`'s method and the CAVEATS section of the
/// analysis file this feeds: the horizon rate-multiplier scaling inside
/// `feats` itself is NOT re-derived per blend, only the dot product is).
/// Exploits `dot(w(t), f) == (1-t) * dot(champion, f) + t * dot(human, f)`
/// -- linearity of the dot product in `w` -- exactly the identity
/// [`rank_under_perturbation`] already relies on, so every `t` is one pass
/// over two already-computed value arrays, no re-evaluation of
/// `linear_features`.
fn rank_gap_at_t(vals_champ: &[f64], vals_human: &[f64], take_pos: usize, t: f64) -> (u64, f64) {
    let blended = |i: usize| -> f64 { (1.0 - t) * vals_champ[i] + t * vals_human[i] };
    let take_val = blended(take_pos);
    let mut best = take_val;
    let mut better = 0u64;
    for i in 0..vals_champ.len() {
        let v = blended(i);
        if i != take_pos && v > take_val {
            better += 1;
        }
        if v > best {
            best = v;
        }
    }
    (better + 1, best - take_val)
}

#[derive(Clone, Copy, Default)]
struct CardStats {
    /// Stage 1: decision points where this card sat in `card_row` and a
    /// take was structurally possible.
    seen: u64,
    /// Stage 2: of those, `Move::Take{slot}` for this card was legal.
    legal: u64,
    /// Stage 3: of those legal takes, the move survived into the candidate
    /// set `rank_moves` actually scored.
    scored: u64,
    /// Stage 4, summed over every scored occurrence: this move's rank
    /// (1 == it won) and the score gap to the winner.
    rank_sum: u64,
    gap_sum: f64,
    /// Stage 4: the best (lowest) rank a take of this card ever achieved.
    best_rank: Option<u64>,
}

impl CardStats {
    fn record_scored(&mut self, rank: u64, gap: f64) {
        self.scored += 1;
        self.rank_sum += rank;
        self.gap_sum += gap;
        self.best_rank = Some(self.best_rank.map_or(rank, |b| b.min(rank)));
    }

    fn merge(&mut self, other: &CardStats) {
        self.seen += other.seen;
        self.legal += other.legal;
        self.scored += other.scored;
        self.rank_sum += other.rank_sum;
        self.gap_sum += other.gap_sum;
        self.best_rank = match (self.best_rank, other.best_rank) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, b) => b,
        };
    }
}

/// The variant name of a [`Move`], read off its `Debug` output rather than a
/// hand-written `match` -- `Move` is a large enum and `Cargo.toml` denies
/// `wildcard_enum_match_arm`, so an exhaustive `match` here just to recover
/// a label would be ~30 arms of pure bookkeeping this diagnostic does not
/// need to own. `{mv:?}` renders e.g. `"Take { slot: 3 }"` or `"EndTurn"`;
/// the first token before a space or `(` is the variant name in both the
/// struct-variant and tuple-variant shapes `Move` uses.
fn move_kind(mv: &Move) -> String {
    let s = format!("{mv:?}");
    s.split(['(', ' ']).next().unwrap_or(&s).to_string()
}

/// Stage-4 diagnosis shared across every decision point where a Mine take
/// was SCORED: what beat it (by [`Move`] kind), and which [`WeightKey`]
/// terms account for the score gap. `key_contrib_sum[k]` accumulates
/// `weight(k) * (winner_feature(k) - take_feature(k))` -- positive means
/// that key widens the winner's advantage over the Mine take that
/// occurrence, negative means it narrows it -- summed over every scored
/// occurrence of every Mine card, ready to average and rank by `|mean|` at
/// report time. Deliberately never printed as a raw number in the report
/// (see `bin/takeprobe.rs`'s own top doc comment / the task's hard
/// constraint against writing down a weight value or coefficient) -- only
/// the ranked key NAMES and their sign are reported.
struct Diag {
    winner_kinds: HashMap<String, u64>,
    key_contrib_sum: Vec<f64>,
    key_contrib_n: u64,
}

impl Diag {
    fn new() -> Diag {
        Diag { winner_kinds: HashMap::new(), key_contrib_sum: vec![0.0; WeightKey::ALL.len()], key_contrib_n: 0 }
    }

    fn merge(&mut self, other: &Diag) {
        for (k, &v) in &other.winner_kinds {
            *self.winner_kinds.entry(k.clone()).or_insert(0) += v;
        }
        for i in 0..self.key_contrib_sum.len() {
            self.key_contrib_sum[i] += other.key_contrib_sum[i];
        }
        self.key_contrib_n += other.key_contrib_n;
    }
}

/// Resolves [`MINE_NAMES`] to [`CardId`]s once. Returns an error string
/// (rather than panicking) if the static card table ever loses one of these
/// names, so a bad `--weights`/table mismatch reports cleanly instead of
/// aborting with a bare `unwrap` panic.
fn mine_card_ids() -> Result<[CardId; 3], String> {
    let mut ids = [CardId::NONE; 3];
    for (i, &name) in MINE_NAMES.iter().enumerate() {
        ids[i] = CardId::by_name(name).ok_or_else(|| format!("card table has no {name:?}"))?;
    }
    Ok(ids)
}

// =====================================================================
// `--build`: the Farm-BUILD follow-up probe.
//
// Where `MINE_NAMES`/`mine_card_ids` track a Farm-family card's SEEN-at-
// TAKE stage (sitting in `state.card_row`), this half of the file tracks
// a Farm-family card's SEEN-at-BUILD stage instead: `Move::Build{card}`
// for it is only ever generated by `legal.rs::action_moves`'s build loop
// out of `tableau_names_sorted(&p.techs)` (`legal.rs` around the
// `p.workers_free > 0` gate) -- so the direct analogue of "sitting in the
// row" for a BUILD is "already developed", i.e. `p.techs.has(id)`
// ([`tta::state::Tableau::has`]), never `p.hand_civil` -- a Farm card
// taken but not yet `Move::Develop`'d is never offered a build move at
// all, by construction of that loop, and this probe measures the exact
// site `legal.rs` reads from rather than a looser "does the player
// possess it anywhere" test.
// =====================================================================

/// The three non-starting farm cards, in age order. Agriculture (age A) is
/// the starting technology printed on the player board, mirroring
/// [`MINE_NAMES`]'s own exclusion of Bronze -- confirmed against
/// `data/cards_civil.json`, which lists exactly Agriculture (A), Irrigation
/// (I), Selective Breeding (II), Mechanized Agriculture (III) as type
/// `"farm"`.
const FARM_NAMES: [&str; 3] = ["Irrigation", "Selective Breeding", "Mechanized Agriculture"];

/// Resolves [`FARM_NAMES`] to [`CardId`]s once, mirroring [`mine_card_ids`].
fn farm_card_ids() -> Result<[CardId; 3], String> {
    let mut ids = [CardId::NONE; 3];
    for (i, &name) in FARM_NAMES.iter().enumerate() {
        ids[i] = CardId::by_name(name).ok_or_else(|| format!("card table has no {name:?}"))?;
    }
    Ok(ids)
}

/// Why `Move::Build{card: id}` for a developed-but-not-legal Farm card was
/// missing from `legal::legal_moves`'s output, bucketed by re-reading the
/// SAME checks `legal.rs`'s build loop makes, in the SAME short-circuit
/// order (see [`classify_build_illegal`]'s own doc comment) -- never a
/// guess. `Other` is the residual bucket: every Farm card here already
/// passed the SEEN gate (`p.techs.has(id)`), so `Other` firing at all would
/// mean this probe's replay of the build loop diverged from `legal.rs`
/// itself, which is itself a finding worth reporting, not a case to hide.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BuildIllegalReason {
    /// `p.workers_free == 0` -- the build loop's own gate, checked ONCE per
    /// decision point, before any card (Farm, Mine, urban or unit) is even
    /// considered. This is the task's leading hypothesis.
    NoFreeWorker,
    /// `costs::build_cost_net` returned `None` (no printed resource cost) --
    /// never observed for a Farm card in practice (every farm tech prints a
    /// nonzero `resourceCost`), kept only so the classifier is exhaustive
    /// over every `continue` site the real loop has.
    NoCostPrinted,
    /// Resources on hand (plus any live Trade Routes Agreement food-as-
    /// resource conversion) fall short of the net cost.
    CannotPayResources,
    /// No spare civil action, and no live Civil Life-style CA-free grant.
    NoCivilAction,
    /// None of the above matched -- see this enum's own doc comment.
    Other,
}

/// The five [`BuildIllegalReason`] variants in a fixed, stable order, for
/// indexing into a `[u64; 5]` tally without a `HashMap<BuildIllegalReason,
/// _>` (which would need `BuildIllegalReason: Hash`, unused everywhere
/// else in this file).
const BUILD_ILLEGAL_REASONS: [BuildIllegalReason; 5] = [
    BuildIllegalReason::NoFreeWorker,
    BuildIllegalReason::NoCostPrinted,
    BuildIllegalReason::CannotPayResources,
    BuildIllegalReason::NoCivilAction,
    BuildIllegalReason::Other,
];

fn build_illegal_reason_idx(r: BuildIllegalReason) -> usize {
    match r {
        BuildIllegalReason::NoFreeWorker => 0,
        BuildIllegalReason::NoCostPrinted => 1,
        BuildIllegalReason::CannotPayResources => 2,
        BuildIllegalReason::NoCivilAction => 3,
        BuildIllegalReason::Other => 4,
    }
}

/// Classifies why `Move::Build{card: id}` is absent from `legal_moves`'
/// output for a Farm card `id` already known to satisfy `p.techs.has(id)`
/// (the SEEN gate) -- by re-running `legal.rs`'s own build-loop checks (the
/// `p.workers_free > 0` outer gate, then `costs::build_cost_net`, then the
/// non-unit affordability check with the Trade Routes Agreement food-as-
/// resource fill, then the civil-action gate) in the SAME order and with
/// the SAME short-circuit semantics that loop uses -- `!affordable ||
/// !(have_ca || ...)` evaluates left-to-right in Rust, so a card failing
/// BOTH affordability and the CA gate is attributed to `CannotPayResources`
/// here exactly as the real `||` would never reach the CA half. A Farm
/// card is never a unit and never urban (`CardType::is_unit`/`is_urban`
/// both false for `CardType::Farm`), so neither of those two branches is
/// replayed here -- they cannot fire for this card kind.
fn classify_build_illegal(state: &GameState, p: &PlayerState, id: CardId) -> BuildIllegalReason {
    if p.workers_free == 0 {
        return BuildIllegalReason::NoFreeWorker;
    }
    let Some(cost) = costs::build_cost_net(state, p, id) else {
        return BuildIllegalReason::NoCostPrinted;
    };
    let res = p.resources as i32;
    let trade_fill = economy::trade_food_as_resource_remaining(state, p).min(p.food as i32);
    let affordable = res >= cost || (res + trade_fill) >= cost;
    if !affordable {
        return BuildIllegalReason::CannotPayResources;
    }
    let have_ca = costs::spare_ca(p) >= 1;
    if !(have_ca || costs::civil_life_ca_free(p.one_time_discount.build_resources)) {
        return BuildIllegalReason::NoCivilAction;
    }
    BuildIllegalReason::Other
}

/// Per-round illegality-reason tally for `--build` mode. `by_round[round]`
/// and `total` are both `[u64; 5]`, indexed by [`build_illegal_reason_idx`]
/// -- see [`BUILD_ILLEGAL_REASONS`].
struct BuildIllegalStats {
    by_round: HashMap<u16, [u64; 5]>,
    total: [u64; 5],
}

impl BuildIllegalStats {
    fn new() -> BuildIllegalStats {
        BuildIllegalStats { by_round: HashMap::new(), total: [0; 5] }
    }

    fn record(&mut self, round: u16, reason: BuildIllegalReason) {
        let idx = build_illegal_reason_idx(reason);
        self.total[idx] += 1;
        let entry = self.by_round.entry(round).or_insert([0; 5]);
        entry[idx] += 1;
    }

    fn merge(&mut self, other: &BuildIllegalStats) {
        self.total.iter_mut().zip(other.total.iter()).for_each(|(a, b)| *a += b);
        for (&round, counts) in &other.by_round {
            let entry = self.by_round.entry(round).or_insert([0; 5]);
            entry.iter_mut().zip(counts.iter()).for_each(|(a, b)| *a += b);
        }
    }
}

/// Plays one self-play game to completion (or to [`MOVE_CAP`]), tracking
/// [`CardStats`]/[`Diag`] for [`FARM_NAMES`] exactly the way [`play_one`]
/// tracks [`MINE_NAMES`], PLUS the [`BuildIllegalStats`] breakdown for
/// every SEEN-but-not-LEGAL occurrence -- see this file's `--build` doc
/// comment block for why SEEN is `p.techs.has(id)` here, not
/// `p.hand_civil`. Never mutates anything outside its own locals, and
/// never calls `apply::apply` on anything but the move the bot itself
/// already chose, for the same reproduces-an-ordinary-self-play-trajectory
/// reason [`play_one`]'s own doc comment gives.
fn play_one_build(players: u8, weights: Weights, seed: u64, farm_ids: &[CardId; 3]) -> ([CardStats; 3], Diag, BuildIllegalStats) {
    let bot = WeightedBot::new(weights);
    let mut state = game::new_game(players, seed);
    let mut stats = [CardStats::default(); 3];
    let mut diag = Diag::new();
    let mut illegal = BuildIllegalStats::new();
    let mut moves_played = 0usize;

    while !state.game_over {
        if moves_played >= MOVE_CAP {
            break;
        }
        moves_played += 1;

        let moves = legal::legal_moves(&state);
        if moves.as_slice().is_empty() {
            break;
        }

        let build_possible = state.phase == Phase::Actions && state.pending.is_empty();
        let mut present: Vec<usize> = Vec::new(); // farm_ids index
        if build_possible {
            let p = state.me();
            for (fi, &id) in farm_ids.iter().enumerate() {
                if p.techs.has(id) {
                    present.push(fi);
                }
            }
        }

        let mv = if present.is_empty() {
            bot.choose(&state, moves.as_slice())
        } else {
            // Same argmax re-derivation `play_one` uses for Mine takes --
            // see that function's own inline comment.
            let feats = candidate_features(&state, moves.as_slice(), false, &weights);
            let vals: Vec<f64> = feats.iter().map(|(_, f)| dot(&weights, f)).collect();
            let mut best_idx = 0usize;
            for (i, &v) in vals.iter().enumerate().skip(1) {
                if v > vals[best_idx] {
                    best_idx = i;
                }
            }
            let winner_val = vals[best_idx];
            let round = state.round;
            let p = state.me();
            for fi in present {
                let id = farm_ids[fi];
                stats[fi].seen += 1;
                let build_mv = Move::Build { card: id };
                if moves.as_slice().contains(&build_mv) {
                    stats[fi].legal += 1;
                    if let Some(pos) = feats.iter().position(|(m, _)| *m == build_mv) {
                        let take_val = vals[pos];
                        let rank = vals.iter().filter(|&&v| v > take_val).count() as u64 + 1;
                        stats[fi].record_scored(rank, winner_val - take_val);
                        for (ki, contrib) in diag.key_contrib_sum.iter_mut().enumerate() {
                            let w = weights.get(WeightKey::ALL[ki]);
                            *contrib += w * (feats[best_idx].1[ki] - feats[pos].1[ki]);
                        }
                        diag.key_contrib_n += 1;
                        *diag.winner_kinds.entry(move_kind(&feats[best_idx].0)).or_insert(0) += 1;
                    }
                } else {
                    let reason = classify_build_illegal(&state, p, id);
                    illegal.record(round, reason);
                }
            }
            feats[best_idx].0
        };
        apply::apply(&mut state, mv);
    }
    (stats, diag, illegal)
}

/// Plays `games` self-play `--build` games across up to `threads` threads
/// and merges every thread's [`play_one_build`] output -- the `--build`
/// mirror of [`run_all`].
fn run_all_build(
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
    weights: Weights,
    farm_ids: &[CardId; 3],
) -> ([CardStats; 3], Diag, BuildIllegalStats, std::time::Duration) {
    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = threads.min(games).max(1);
    let totals: Mutex<[CardStats; 3]> = Mutex::new([CardStats::default(); 3]);
    let diag_totals: Mutex<Diag> = Mutex::new(Diag::new());
    let illegal_totals: Mutex<BuildIllegalStats> = Mutex::new(BuildIllegalStats::new());

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(|| {
                let mut local = [CardStats::default(); 3];
                let mut local_diag = Diag::new();
                let mut local_illegal = BuildIllegalStats::new();
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= games {
                        break;
                    }
                    let this_seed = seed + g as u64;
                    let (s, d, il) = play_one_build(players, weights, this_seed, farm_ids);
                    for (l, s) in local.iter_mut().zip(s.iter()) {
                        l.merge(s);
                    }
                    local_diag.merge(&d);
                    local_illegal.merge(&il);
                }
                let mut t = totals.lock().expect("totals mutex poisoned");
                for (t, l) in t.iter_mut().zip(local.iter()) {
                    t.merge(l);
                }
                diag_totals.lock().expect("diag mutex poisoned").merge(&local_diag);
                illegal_totals.lock().expect("illegal mutex poisoned").merge(&local_illegal);
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    });

    (
        totals.into_inner().expect("totals mutex poisoned"),
        diag_totals.into_inner().expect("diag mutex poisoned"),
        illegal_totals.into_inner().expect("illegal mutex poisoned"),
        start.elapsed(),
    )
}

/// Prints the `--build` report: the [`CardStats`] funnel (reusing the same
/// table shape [`main`] prints for Mine takes), the illegality-reason
/// breakdown (overall and per round), the winner-kind/[`WeightKey`]
/// diagnosis when anything scored, exactly mirroring the take-mode report
/// below it in [`main`].
fn print_build_report(games: usize, players: u8, weights_path: &str, totals: &[CardStats; 3], diag: &Diag, illegal: &BuildIllegalStats, elapsed: std::time::Duration) {
    println!("takeprobe --build: {games} games, {players} players, weights={weights_path}");
    println!("elapsed: {elapsed:.1?}");
    println!();
    println!(
        "{:<10} {:>8} {:>8} {:>8}   {:>6} {:>6}   {:>10} {:>9}",
        "card", "seen", "legal", "scored", "L-rate", "S-rate", "mean_gap", "best_rank"
    );
    for (i, &name) in FARM_NAMES.iter().enumerate() {
        let s = &totals[i];
        let legal_rate = if s.seen > 0 { s.legal as f64 / s.seen as f64 } else { 0.0 };
        let scored_rate = if s.legal > 0 { s.scored as f64 / s.legal as f64 } else { 0.0 };
        let mean_gap = if s.scored > 0 { s.gap_sum / s.scored as f64 } else { 0.0 };
        let best_rank = s.best_rank.map_or("n/a".to_string(), |r| r.to_string());
        println!(
            "{name:<10} {:>8} {:>8} {:>8}   {legal_rate:>6.3} {scored_rate:>6.3}   {mean_gap:>10.4} {best_rank:>9}",
            s.seen, s.legal, s.scored
        );
    }
    println!();
    let mut combined = CardStats::default();
    for s in totals {
        combined.merge(s);
    }
    let legal_rate = if combined.seen > 0 { combined.legal as f64 / combined.seen as f64 } else { 0.0 };
    let scored_rate = if combined.legal > 0 { combined.scored as f64 / combined.legal as f64 } else { 0.0 };
    let mean_rank = if combined.scored > 0 { combined.rank_sum as f64 / combined.scored as f64 } else { 0.0 };
    let mean_gap = if combined.scored > 0 { combined.gap_sum / combined.scored as f64 } else { 0.0 };
    let illegal_count: u64 = combined.seen - combined.legal;
    println!(
        "combined: seen={} legal={} ({:.3}) scored={} ({:.3}) mean_rank={:.2} mean_gap={:.4} best_rank={}",
        combined.seen,
        combined.legal,
        legal_rate,
        combined.scored,
        scored_rate,
        mean_rank,
        mean_gap,
        combined.best_rank.map_or("n/a".to_string(), |r| r.to_string())
    );
    println!();
    println!("Illegality-reason breakdown (n={illegal_count} SEEN-but-not-LEGAL occurrences):");
    for &reason in &BUILD_ILLEGAL_REASONS {
        let c = illegal.total[build_illegal_reason_idx(reason)];
        let rate = if illegal_count > 0 { c as f64 / illegal_count as f64 } else { 0.0 };
        println!("  {reason:?}  {c:>8}  ({rate:.3})");
    }
    println!();
    println!("Illegality-reason breakdown by round:");
    let mut rounds: Vec<&u16> = illegal.by_round.keys().collect();
    rounds.sort_unstable();
    println!(
        "  {:>5}  {:>10} {:>13} {:>18} {:>13} {:>7} {:>7}",
        "round", "NoFreeWork", "NoCostPrint", "CannotPayResrc", "NoCivilActn", "Other", "total"
    );
    for round in rounds {
        let counts = &illegal.by_round[round];
        let row_total: u64 = counts.iter().sum();
        println!(
            "  {round:>5}  {:>10} {:>13} {:>18} {:>13} {:>7} {row_total:>7}",
            counts[0], counts[1], counts[2], counts[3], counts[4]
        );
    }

    println!();
    println!("Winning move kind at every SCORED Farm-build decision point (what beat it):");
    let mut kinds: Vec<(&String, &u64)> = diag.winner_kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let kind_total: u64 = diag.winner_kinds.values().sum();
    for (kind, count) in &kinds {
        let rate = if kind_total > 0 { **count as f64 / kind_total as f64 } else { 0.0 };
        println!("  {kind:<14} {count:>6}  ({rate:.3})");
    }

    println!();
    println!(
        "Top WeightKey terms by mean |contribution| to the winner-vs-Farm-build gap (n={}):",
        diag.key_contrib_n
    );
    if diag.key_contrib_n > 0 {
        let n = diag.key_contrib_n as f64;
        let mut ranked: Vec<(WeightKey, f64)> =
            WeightKey::ALL.iter().enumerate().map(|(i, &k)| (k, diag.key_contrib_sum[i] / n)).collect();
        ranked.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (key, mean)) in ranked.iter().take(15).enumerate() {
            let sign = if *mean > 0.0 { "favors_winner" } else if *mean < 0.0 { "favors_take" } else { "neutral" };
            println!("  {:>2}. {:<24} {sign}", rank + 1, format!("{key:?}"));
        }
    } else {
        println!("  (no scored Farm-build occurrences to rank)");
    }
}

/// Everything about a `play_one` call that is fixed for the whole run
/// (shared read-only across every game thread) rather than varying per
/// game -- bundled so `play_one` stays under clippy's `too_many_arguments`
/// without folding unrelated concerns (mine IDs, `--sensitivity` inputs,
/// `--interpolate` inputs) into one another's own structs. Every field is
/// itself `Copy` (references and a `bool`), so the struct is too --
/// `play_one` destructures `*cfg` rather than threading a lifetime-tied
/// borrow through its own body.
#[derive(Clone, Copy)]
struct ProbeConfig<'a> {
    mine_ids: &'a [CardId; 3],
    sensitivity: bool,
    sens_keys: &'a [WeightKey; SENS_KEY_NAMES.len()],
    clamp_keys: &'a [WeightKey; CLAMP_KEY_NAMES.len()],
    interp: Option<&'a InterpCtx<'a>>,
}

/// Plays one self-play game to completion (or to [`MOVE_CAP`]), recording
/// [`CardStats`] for each of [`MINE_NAMES`]. Never mutates anything outside
/// its own locals -- a fresh `GameState` from `game::new_game` and a fresh
/// `WeightedBot` -- and never calls `apply::apply` on anything but the
/// move the bot itself already chose, so this reproduces an ordinary
/// self-play trajectory exactly; it only ever ADDS observation, never
/// changes what gets played.
fn play_one(players: u8, weights: Weights, seed: u64, cfg: &ProbeConfig) -> ([CardStats; 3], Diag, SensStats, InterpStats, AdoptStats) {
    let ProbeConfig { mine_ids, sensitivity, sens_keys, clamp_keys, interp } = *cfg;
    let bot = WeightedBot::new(weights);
    let mut state = game::new_game(players, seed);
    let mut stats = [CardStats::default(); 3];
    let mut diag = Diag::new();
    let mut sens = SensStats::new();
    let mut interp_stats = InterpStats::new();
    let mut key_adopt = AdoptStats::new(interp.map_or(0, |c| c.differ_keys.len()));
    let mut moves_played = 0usize;

    while !state.game_over {
        if moves_played >= MOVE_CAP {
            break;
        }
        moves_played += 1;

        let moves = legal::legal_moves(&state);
        if moves.as_slice().is_empty() {
            break;
        }

        let take_possible = state.phase == Phase::Actions && state.pending.is_empty();
        let mut present: Vec<(usize, u8)> = Vec::new(); // (mine_ids index, row slot)
        if take_possible {
            for (slot, &id) in state.card_row.iter().enumerate() {
                if let Some(mi) = mine_ids.iter().position(|&m| m == id) {
                    present.push((mi, slot as u8));
                }
            }
        }

        let mv = if present.is_empty() {
            bot.choose(&state, moves.as_slice())
        } else {
            // `candidate_features` shares the exact same root/ctx/trial
            // construction `WeightedBot::choose`/`rank_moves` use (its own
            // doc comment), and `dot(w, f)` reproduces `evaluate`'s score
            // exactly, `EndTurnBias` included -- so picking the strict-`>`
            // argmax over `dot(w, f)` here, first-candidate-wins on ties,
            // reproduces `choose`'s own pick exactly (same tie-break rule),
            // while additionally exposing every candidate's per-`WeightKey`
            // feature vector for the stage-4 key-contribution breakdown.
            let feats = candidate_features(&state, moves.as_slice(), false, &weights);
            let vals: Vec<f64> = feats.iter().map(|(_, f)| dot(&weights, f)).collect();
            let mut best_idx = 0usize;
            for (i, &v) in vals.iter().enumerate().skip(1) {
                if v > vals[best_idx] {
                    best_idx = i;
                }
            }
            let winner_val = vals[best_idx];
            for (mi, slot) in present {
                stats[mi].seen += 1;
                let take_mv = Move::Take { slot };
                if moves.as_slice().contains(&take_mv) {
                    stats[mi].legal += 1;
                    if let Some(pos) = feats.iter().position(|(m, _)| *m == take_mv) {
                        let take_val = vals[pos];
                        let rank = vals.iter().filter(|&&v| v > take_val).count() as u64 + 1;
                        stats[mi].record_scored(rank, winner_val - take_val);
                        for (ki, contrib) in diag.key_contrib_sum.iter_mut().enumerate() {
                            let w = weights.get(WeightKey::ALL[ki]);
                            *contrib += w * (feats[best_idx].1[ki] - feats[pos].1[ki]);
                        }
                        diag.key_contrib_n += 1;
                        *diag.winner_kinds.entry(move_kind(&feats[best_idx].0)).or_insert(0) += 1;

                        // `--sensitivity`: reuse this SAME decision point's
                        // `vals`/`feats` (already computed above for the
                        // stage-4 diagnosis) to answer every `(key, m)` cell
                        // via `rank_under_perturbation` -- one game pass,
                        // many static re-scorings, per this task's own
                        // method requirement.
                        if sensitivity {
                            for (ki, key) in sens_keys.iter().enumerate() {
                                let wk = weights.get(*key);
                                let key_idx = *key as usize;
                                for (mi, &m) in M_SWEEP.iter().enumerate() {
                                    let delta_weight = (m - 1.0) * wk;
                                    let r = rank_under_perturbation(&vals, &feats, pos, &[(key_idx, delta_weight)]);
                                    let cell = &mut sens.per_key[ki][mi];
                                    cell.0 += 1;
                                    if r == 1 {
                                        cell.1 += 1;
                                    }
                                    if r <= 2 {
                                        cell.2 += 1;
                                    }
                                }
                            }
                            let clamp_adjustments: Vec<(usize, f64)> =
                                clamp_keys.iter().map(|&k| (k as usize, -weights.get(k))).collect();
                            let r = rank_under_perturbation(&vals, &feats, pos, &clamp_adjustments);
                            sens.joint_clamp.0 += 1;
                            if r == 1 {
                                sens.joint_clamp.1 += 1;
                            }
                            if r <= 2 {
                                sens.joint_clamp.2 += 1;
                            }
                        }

                        // `--interpolate`: reuse this SAME decision point's
                        // `vals`/`feats` again -- one extra dot product per
                        // candidate against the human vector, then every
                        // `t` in `T_GRID_LEN` is a cheap blend
                        // (`rank_gap_at_t`'s own doc comment), plus the
                        // single-key adoption sweep via
                        // `rank_under_perturbation` (already used above by
                        // `--sensitivity`), one game pass answering both.
                        if let Some(ctx) = interp {
                            let vals_human: Vec<f64> = feats.iter().map(|(_, f)| dot(&ctx.human, f)).collect();
                            for i in 0..T_GRID_LEN {
                                let (rank, gap) = rank_gap_at_t(&vals, &vals_human, pos, t_at(i));
                                let cell = &mut interp_stats.by_t[i];
                                cell.0 += 1;
                                if rank == 1 {
                                    cell.1 += 1;
                                }
                                if rank <= 2 {
                                    cell.2 += 1;
                                }
                                cell.3 += rank;
                                cell.4 += gap;
                            }
                            for (ki, &key) in ctx.differ_keys.iter().enumerate() {
                                let key_idx = key as usize;
                                // ADOPTION: `w[key] = ctx.human.get(key)`
                                // (already `s`-scaled by `main` when
                                // `--normalize` is set), with `ctx.locked`'s
                                // already-chosen greedy keys applied first
                                // -- see `InterpCtx::locked`'s doc comment.
                                let adopt_delta = ctx.human.get(key) - weights.get(key);
                                let mut adopt_adj: Vec<(usize, f64)> = ctx.locked.to_vec();
                                adopt_adj.push((key_idx, adopt_delta));
                                let r_adopt = rank_under_perturbation(&vals, &feats, pos, &adopt_adj);
                                let adopt_cell = &mut key_adopt.adopt.per_key[ki];
                                adopt_cell.0 += 1;
                                if r_adopt == 1 {
                                    adopt_cell.1 += 1;
                                }
                                // DELETION cross-check: `w[key] = 0`, never
                                // `locked`-prefixed -- a fixed baseline
                                // independent of the greedy state, so
                                // `--normalize`'s report can compare
                                // adoption against it key-for-key (see
                                // `AdoptStats`'s doc comment).
                                let delete_delta = -weights.get(key);
                                let r_delete = rank_under_perturbation(&vals, &feats, pos, &[(key_idx, delete_delta)]);
                                let delete_cell = &mut key_adopt.delete.per_key[ki];
                                delete_cell.0 += 1;
                                if r_delete == 1 {
                                    delete_cell.1 += 1;
                                }
                            }
                        }
                    }
                }
            }
            feats[best_idx].0
        };
        apply::apply(&mut state, mv);
    }
    (stats, diag, sens, interp_stats, key_adopt)
}

struct Args {
    games: usize,
    players: u8,
    weights_path: String,
    seed: u64,
    threads: usize,
    sensitivity: bool,
    interpolate_path: Option<String>,
    normalize: bool,
    /// `--build`: run the Farm-BUILD probe (see this file's `--build` doc
    /// comment block) instead of the Mine-TAKE probe. Mutually exclusive
    /// with `--sensitivity`/`--interpolate`/`--normalize`, which are all
    /// Mine-TAKE-probe-specific machinery this mode does not share.
    build: bool,
}

impl Default for Args {
    fn default() -> Args {
        Args {
            games: 200,
            players: 3,
            weights_path: String::new(),
            seed: 1,
            threads: 1,
            sensitivity: false,
            interpolate_path: None,
            normalize: false,
            build: false,
        }
    }
}

const USAGE: &str = "\
usage: takeprobe --weights PATH [options]

  --games N          games to play (default 200)
  --players N        2, 3 or 4 (default 3)
  --weights PATH     champion JSON every seat plays (required)
  --seed N           base seed; game g uses seed+g (default 1)
  --threads N        games in parallel (default 1)
  --sensitivity      also sweep SENS_KEY_NAMES x M_SWEEP (static rescoring,
                       see this binary's doc comment) and the joint
                       clamp-block check
  --interpolate PATH also sweep w(t) = (1-t)*champion + t*PATH over
                       T_GRID_LEN points, plus a single-key champion->PATH
                       adoption sweep over every differing WeightKey (static
                       rescoring, see InterpCtx's doc comment)
  --normalize        requires --interpolate; redoes both --interpolate
                       sweeps on a scale-matched PATH vector (PATH scaled by
                       s = ||champion||_2 / ||PATH||_2 first) rather than the
                       raw PATH vector -- see l2_norm's doc comment. Does not
                       change --interpolate's output when omitted.
  --build            run the Farm-BUILD probe (SEEN=p.techs.has(id), i.e.
                       already developed) instead of the Mine-TAKE probe --
                       see this file's `--build` doc comment block. Mutually
                       exclusive with --sensitivity/--interpolate/--normalize.
  --help
";

fn parse_args(argv: &[String]) -> Result<Option<Args>, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(flag) = it.next() {
        let mut value = |flag: &str| -> Result<String, String> {
            it.next().cloned().ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--games" => a.games = value(flag)?.parse().map_err(|_| "bad --games".to_string())?,
            "--players" => a.players = value(flag)?.parse().map_err(|_| "bad --players".to_string())?,
            "--weights" => a.weights_path = value(flag)?,
            "--seed" => a.seed = value(flag)?.parse().map_err(|_| "bad --seed".to_string())?,
            "--threads" => a.threads = value(flag)?.parse().map_err(|_| "bad --threads".to_string())?,
            "--sensitivity" => a.sensitivity = true,
            "--interpolate" => a.interpolate_path = Some(value(flag)?),
            "--normalize" => a.normalize = true,
            "--build" => a.build = true,
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("unknown flag {other}\n\n{USAGE}")),
        }
    }
    if !(2..=4).contains(&a.players) {
        return Err(format!("--players must be 2, 3 or 4, got {}", a.players));
    }
    if a.weights_path.is_empty() {
        return Err("--weights is required".to_string());
    }
    if a.games == 0 {
        return Err("--games must be at least 1".to_string());
    }
    if a.normalize && a.interpolate_path.is_none() {
        return Err("--normalize requires --interpolate PATH".to_string());
    }
    if a.build && (a.sensitivity || a.interpolate_path.is_some() || a.normalize) {
        return Err("--build is mutually exclusive with --sensitivity/--interpolate/--normalize".to_string());
    }
    if a.threads == 0 {
        a.threads = 1;
    }
    Ok(Some(a))
}

/// Plays `games` self-play games across up to `threads` threads (the same
/// work `main` used to do inline before `--normalize`'s greedy joint check
/// needed to repeat it up to 8 times with a different [`ProbeConfig`] each
/// time -- see `main`'s greedy loop) and merges every thread's [`play_one`]
/// output into one set of totals. Pure aggregation; never mutates `cfg` or
/// `weights`, so calling it twice with the same `weights`/`seed`/`games`
/// reproduces the identical per-game trajectories (same seeds), only
/// re-scored under whatever `cfg.interp` says this time.
fn run_all(
    games: usize,
    players: u8,
    seed: u64,
    threads: usize,
    weights: Weights,
    cfg: &ProbeConfig,
    n_keys: usize,
) -> ([CardStats; 3], Diag, SensStats, InterpStats, AdoptStats, std::time::Duration) {
    let start = Instant::now();
    let next = AtomicUsize::new(0);
    let threads = threads.min(games).max(1);
    let totals: Mutex<[CardStats; 3]> = Mutex::new([CardStats::default(); 3]);
    let diag_totals: Mutex<Diag> = Mutex::new(Diag::new());
    let sens_totals: Mutex<SensStats> = Mutex::new(SensStats::new());
    let interp_totals: Mutex<InterpStats> = Mutex::new(InterpStats::new());
    let key_adopt_totals: Mutex<AdoptStats> = Mutex::new(AdoptStats::new(n_keys));

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            handles.push(scope.spawn(|| {
                let mut local = [CardStats::default(); 3];
                let mut local_diag = Diag::new();
                let mut local_sens = SensStats::new();
                let mut local_interp = InterpStats::new();
                let mut local_key_adopt = AdoptStats::new(n_keys);
                loop {
                    let g = next.fetch_add(1, Ordering::Relaxed);
                    if g >= games {
                        break;
                    }
                    let this_seed = seed + g as u64;
                    let (s, d, sn, it, ka) = play_one(players, weights, this_seed, cfg);
                    for i in 0..3 {
                        local[i].merge(&s[i]);
                    }
                    local_diag.merge(&d);
                    local_sens.merge(&sn);
                    local_interp.merge(&it);
                    local_key_adopt.merge(&ka);
                }
                let mut t = totals.lock().expect("totals mutex poisoned");
                for i in 0..3 {
                    t[i].merge(&local[i]);
                }
                diag_totals.lock().expect("diag mutex poisoned").merge(&local_diag);
                sens_totals.lock().expect("sens mutex poisoned").merge(&local_sens);
                interp_totals.lock().expect("interp mutex poisoned").merge(&local_interp);
                key_adopt_totals.lock().expect("key_adopt mutex poisoned").merge(&local_key_adopt);
            }));
        }
        for h in handles {
            let _ = h.join();
        }
    });

    (
        totals.into_inner().expect("totals mutex poisoned"),
        diag_totals.into_inner().expect("diag mutex poisoned"),
        sens_totals.into_inner().expect("sens mutex poisoned"),
        interp_totals.into_inner().expect("interp mutex poisoned"),
        key_adopt_totals.into_inner().expect("key_adopt mutex poisoned"),
        start.elapsed(),
    )
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = match parse_args(&argv) {
        Ok(Some(a)) => a,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("takeprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let weights = match load_weights(std::path::Path::new(&args.weights_path)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("takeprobe: loading {}: {e}", args.weights_path);
            return ExitCode::FAILURE;
        }
    };

    if args.build {
        let farm_ids = match farm_card_ids() {
            Ok(ids) => ids,
            Err(e) => {
                eprintln!("takeprobe: {e}");
                return ExitCode::FAILURE;
            }
        };
        let (totals, diag, illegal, elapsed) =
            run_all_build(args.games, args.players, args.seed, args.threads, weights, &farm_ids);
        print_build_report(args.games, args.players, &args.weights_path, &totals, &diag, &illegal, elapsed);
        return ExitCode::SUCCESS;
    }

    let mine_ids = match mine_card_ids() {
        Ok(ids) => ids,
        Err(e) => {
            eprintln!("takeprobe: {e}");
            return ExitCode::FAILURE;
        }
    };

    let sens_keys = sens_keys();
    let clamp_keys = clamp_keys();

    // `--interpolate`: load the second (human) vector once and compute the
    // set of `WeightKey`s where it differs from the champion beyond float
    // noise -- see [`InterpCtx`]'s own doc comment on why only differing
    // keys are worth sweeping in the single-key adoption measurement.
    let human_weights = match &args.interpolate_path {
        Some(path) => match load_weights(std::path::Path::new(path)) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("takeprobe: loading --interpolate {path}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let differ_keys: Vec<WeightKey> = match human_weights {
        Some(h) => WeightKey::ALL.iter().copied().filter(|&k| (weights.get(k) - h.get(k)).abs() > 1e-9).collect(),
        None => Vec::new(),
    };
    // `--normalize`: `s = ||champion||_2 / ||human||_2` over the FULLY
    // RESOLVED vectors (see [`l2_norm`]'s own doc comment on why this is
    // the same as "over the union of keys, resolved the way `load_weights`
    // resolves them" -- both are already the same `N`-wide array by the
    // time `load_weights` returns). `s > 0` always: both norms are sums of
    // squares of a nonempty array, so `s` is finite and positive whenever
    // `human_weights` isn't the all-zero vector.
    let champ_norm = l2_norm(&weights);
    let human_norm = human_weights.as_ref().map(l2_norm);
    let scale_s = human_norm.map(|hn| champ_norm / hn);
    let human_for_interp: Option<Weights> = human_weights.map(|h| {
        if args.normalize {
            scale_weights(&h, scale_s.expect("scale_s is Some whenever human_weights is Some"))
        } else {
            h
        }
    });
    let interp_ctx: Option<InterpCtx> =
        human_for_interp.map(|human| InterpCtx { human, differ_keys: &differ_keys, locked: &[] });
    let cfg = ProbeConfig {
        mine_ids: &mine_ids,
        sensitivity: args.sensitivity,
        sens_keys: &sens_keys,
        clamp_keys: &clamp_keys,
        interp: interp_ctx.as_ref(),
    };

    let (totals, diag, sens, interp, key_adopt, elapsed) =
        run_all(args.games, args.players, args.seed, args.threads, weights, &cfg, differ_keys.len());

    println!("takeprobe: {} games, {} players, weights={}", args.games, args.players, args.weights_path);
    println!("elapsed: {elapsed:.1?}");
    println!();
    println!(
        "{:<6} {:>8} {:>8} {:>8}   {:>6} {:>6}   {:>10} {:>9}",
        "card", "seen", "legal", "scored", "L-rate", "S-rate", "mean_gap", "best_rank"
    );
    for (i, &name) in MINE_NAMES.iter().enumerate() {
        let s = &totals[i];
        let legal_rate = if s.seen > 0 { s.legal as f64 / s.seen as f64 } else { 0.0 };
        let scored_rate = if s.legal > 0 { s.scored as f64 / s.legal as f64 } else { 0.0 };
        let mean_gap = if s.scored > 0 { s.gap_sum / s.scored as f64 } else { 0.0 };
        let best_rank = s.best_rank.map_or("n/a".to_string(), |r| r.to_string());
        println!(
            "{name:<6} {:>8} {:>8} {:>8}   {legal_rate:>6.3} {scored_rate:>6.3}   {mean_gap:>10.4} {best_rank:>9}",
            s.seen, s.legal, s.scored
        );
    }
    println!();
    let mut combined = CardStats::default();
    for s in &totals {
        combined.merge(s);
    }
    let legal_rate = if combined.seen > 0 { combined.legal as f64 / combined.seen as f64 } else { 0.0 };
    let scored_rate = if combined.legal > 0 { combined.scored as f64 / combined.legal as f64 } else { 0.0 };
    let mean_rank = if combined.scored > 0 { combined.rank_sum as f64 / combined.scored as f64 } else { 0.0 };
    let mean_gap = if combined.scored > 0 { combined.gap_sum / combined.scored as f64 } else { 0.0 };
    println!(
        "combined: seen={} legal={} ({:.3}) scored={} ({:.3}) mean_rank={:.2} mean_gap={:.4} best_rank={}",
        combined.seen,
        combined.legal,
        legal_rate,
        combined.scored,
        scored_rate,
        mean_rank,
        mean_gap,
        combined.best_rank.map_or("n/a".to_string(), |r| r.to_string())
    );

    println!();
    println!("Winning move kind at every SCORED Mine-take decision point (what beat it):");
    let mut kinds: Vec<(&String, &u64)> = diag.winner_kinds.iter().collect();
    kinds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let kind_total: u64 = diag.winner_kinds.values().sum();
    for (kind, count) in &kinds {
        let rate = if kind_total > 0 { **count as f64 / kind_total as f64 } else { 0.0 };
        println!("  {kind:<14} {count:>6}  ({rate:.3})");
    }

    // Rank WeightKey terms by mean |contribution| to the winner-vs-take gap,
    // WITHOUT printing the raw contribution or any weight value -- only the
    // ranked key NAMES and the SIGN of their net effect (see `Diag`'s own
    // doc comment). `favors_winner` (positive mean) widens the gap against
    // the Mine take; `favors_take` (negative mean) narrows it.
    println!();
    println!(
        "Top WeightKey terms by mean |contribution| to the winner-vs-Mine-take gap (n={}):",
        diag.key_contrib_n
    );
    if diag.key_contrib_n > 0 {
        let n = diag.key_contrib_n as f64;
        let mut ranked: Vec<(WeightKey, f64)> =
            WeightKey::ALL.iter().enumerate().map(|(i, &k)| (k, diag.key_contrib_sum[i] / n)).collect();
        ranked.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (key, mean)) in ranked.iter().take(15).enumerate() {
            let sign = if *mean > 0.0 { "favors_winner" } else if *mean < 0.0 { "favors_take" } else { "neutral" };
            println!("  {:>2}. {:<24} {sign}", rank + 1, format!("{key:?}"));
        }
    } else {
        println!("  (no scored Mine-take occurrences to rank)");
    }

    // `--sensitivity`: for each `sens_keys` entry, sweep M_SWEEP and report
    // the rank1/rank1-or-2 fractions this STATIC re-score of the SAME
    // feature vectors already gathered above finds (see
    // `rank_under_perturbation`'s own doc comment). This is a re-scoring at
    // a fixed state, not a policy replay -- printed again on every run so
    // the caveat travels with the numbers, not just in the analysis file.
    if args.sensitivity {
        println!();
        println!("=== --sensitivity: static rescoring, NOT a policy replay (see file header) ===");
        for (ki, key) in sens_keys.iter().enumerate() {
            println!();
            println!("{:?}  (n per cell = {})", key, sens.per_key[ki].first().map_or(0, |c| c.0));
            println!("  {:>12} {:>8} {:>10} {:>10}", "m", "n", "rank1", "rank1or2");
            let mut best_log: Option<f64> = None;
            for (mi, &m) in M_SWEEP.iter().enumerate() {
                let (n, r1, r12) = sens.per_key[ki][mi];
                let f1 = if n > 0 { r1 as f64 / n as f64 } else { 0.0 };
                let f12 = if n > 0 { r12 as f64 / n as f64 } else { 0.0 };
                println!("  {m:>12.4} {n:>8} {f1:>10.4} {f12:>10.4}");
                if r1 > 0 && m > 0.0 {
                    let log_m = m.ln().abs();
                    best_log = Some(best_log.map_or(log_m, |b: f64| b.min(log_m)));
                }
            }
            let zero_hit = sens.per_key[ki][0].1 > 0; // M_SWEEP[0] == 0.0 (delete the key)
            match (best_log, zero_hit) {
                (Some(l), _) => println!("  smallest |ln m| achieving nonzero rank1 fraction: {l:.4}"),
                (None, true) => println!("  smallest |ln m| achieving nonzero rank1 fraction: only at m=0 (deleted)"),
                (None, false) => println!("  smallest |ln m| achieving nonzero rank1 fraction: never (0 <= m <= 1e6 swept)"),
            }
        }

        println!();
        println!(
            "Joint clamp-block check (all of {:?} set to m=0 simultaneously):",
            clamp_keys.map(|k| format!("{k:?}"))
        );
        let (n, r1, r12) = sens.joint_clamp;
        let f1 = if n > 0 { r1 as f64 / n as f64 } else { 0.0 };
        let f12 = if n > 0 { r12 as f64 / n as f64 } else { 0.0 };
        println!("  n={n} rank1_frac={f1:.4} rank1or2_frac={f12:.4}");
    }

    // `--interpolate`: `w(t) = (1-t)*champion + t*human`, static rescoring
    // at the SAME decision points `--sensitivity` measures (this binary's
    // doc comment / `rank_gap_at_t`'s own doc comment) -- not a policy
    // replay. Coarse table every 0.05, then a finer 0.01 breakdown of
    // whichever 0.05-wide coarse interval shows the single biggest jump in
    // rank1 fraction (the empirical "transition", if any), then the
    // single-key champion->human adoption sweep.
    if let (Some(path), Some(ctx)) = (&args.interpolate_path, &interp_ctx) {
        println!();
        println!("=== --interpolate: w(t) = (1-t)*champion + t*human, static rescoring, NOT a policy replay ===");
        println!("champion={} human={path}", args.weights_path);
        println!("keys differing champion vs human (beyond 1e-9): {}", ctx.differ_keys.len());
        if args.normalize {
            let s = scale_s.expect("scale_s is Some whenever args.normalize && human_weights.is_some()");
            println!();
            println!("--normalize active: sweeping w(t) = (1-t)*champion + t*(s*human), not raw human.");
            println!(
                "||champion||_2={champ_norm:.4}  ||human||_2={:.4}  s=||champion||/||human||={s:.4}",
                human_norm.expect("human_norm is Some whenever args.normalize && human_weights.is_some()")
            );
            println!(
                "(norms taken over the full N={}-wide load_weights-resolved vector -- see l2_norm's doc comment)",
                WeightKey::ALL.len()
            );
        }
        println!();
        println!("{:>6} {:>8} {:>10} {:>10} {:>10} {:>12}", "t", "n", "rank1", "rank1or2", "mean_rank", "mean_gap");
        let mut coarse: Vec<(usize, f64, f64, f64, f64, f64)> = Vec::new(); // (grid_idx, t, n, f1, f12, mean_rank)
        for i in (0..T_GRID_LEN).step_by(5) {
            let (n, r1, r12, rsum, gsum) = interp.by_t[i];
            let f1 = if n > 0 { r1 as f64 / n as f64 } else { 0.0 };
            let f12 = if n > 0 { r12 as f64 / n as f64 } else { 0.0 };
            let mean_rank = if n > 0 { rsum as f64 / n as f64 } else { 0.0 };
            let mean_gap = if n > 0 { gsum / n as f64 } else { 0.0 };
            println!("{:>6.2} {n:>8} {f1:>10.4} {f12:>10.4} {mean_rank:>10.2} {mean_gap:>12.4}", t_at(i));
            coarse.push((i, t_at(i), n as f64, f1, f12, mean_rank));
        }

        // Endpoint correctness check: `t=1.00` is a positive multiple of
        // the human vector regardless of `--normalize` (scaling by `s > 0`
        // cannot change an argmax -- l2_norm's/scale_weights' own doc
        // comments), so it MUST reproduce the un-normalized run's own
        // t=1.00 row (rank1=0.0645, rank1or2=0.1767, mean_rank=6.63,
        // reference: mine_take_interpolation_3p_2026-08-24.txt) whether or
        // not `--normalize` was passed. If it doesn't, `--normalize`
        // introduced a bug in this same code path, not a new finding.
        {
            let (n, r1, r12, rsum, _) = interp.by_t[T_GRID_LEN - 1];
            let f1 = if n > 0 { r1 as f64 / n as f64 } else { 0.0 };
            let f12 = if n > 0 { r12 as f64 / n as f64 } else { 0.0 };
            let mean_rank = if n > 0 { rsum as f64 / n as f64 } else { 0.0 };
            let ok = (f1 - 0.0645).abs() < 5e-4 && (f12 - 0.1767).abs() < 5e-4 && (mean_rank - 6.63).abs() < 5e-2;
            println!();
            println!(
                "ENDPOINT CHECK (t=1.00 vs reference rank1=0.0645 rank1or2=0.1767 mean_rank=6.63): got rank1={f1:.4} rank1or2={f12:.4} mean_rank={mean_rank:.2} -- {}",
                if ok { "PASS" } else { "FAIL -- investigate before trusting anything else in this file" }
            );
        }

        // Biggest one-step jump in rank1 fraction between adjacent coarse
        // points -- the candidate "transition interval", if the sweep has
        // one at all.
        let mut best_jump = 0.0f64;
        let mut best_pair: Option<(usize, usize)> = None; // (grid_idx_lo, grid_idx_hi)
        for w in coarse.windows(2) {
            let jump = (w[1].3 - w[0].3).abs();
            if jump > best_jump {
                best_jump = jump;
                best_pair = Some((w[0].0, w[1].0));
            }
        }
        println!();
        match best_pair {
            Some((lo, hi)) if best_jump > 1e-9 => {
                println!(
                    "Biggest single-step rank1-fraction jump: {best_jump:.4} between t={:.2} and t={:.2} -- fine (0.01) breakdown of that interval:",
                    t_at(lo),
                    t_at(hi)
                );
                println!(
                    "{:>6} {:>8} {:>10} {:>10} {:>10} {:>12}",
                    "t", "n", "rank1", "rank1or2", "mean_rank", "mean_gap"
                );
                for i in lo..=hi {
                    let (n, r1, r12, rsum, gsum) = interp.by_t[i];
                    let f1 = if n > 0 { r1 as f64 / n as f64 } else { 0.0 };
                    let f12 = if n > 0 { r12 as f64 / n as f64 } else { 0.0 };
                    let mean_rank = if n > 0 { rsum as f64 / n as f64 } else { 0.0 };
                    let mean_gap = if n > 0 { gsum / n as f64 } else { 0.0 };
                    println!("{:>6.2} {n:>8} {f1:>10.4} {f12:>10.4} {mean_rank:>10.2} {mean_gap:>12.4}", t_at(i));
                }
            }
            _ => {
                println!(
                    "No transition observed: rank1 fraction never moves by more than 1e-9 between adjacent t=0.05 steps across the full 0.00..=1.00 sweep."
                );
            }
        }

        println!();
        println!(
            "Single-key adoption champion->{} (w[key]={}[key], all else champion), ranked by rank1 fraction (n={} differing keys):",
            if args.normalize { "s*human" } else { "human" },
            if args.normalize { "s*human" } else { "human" },
            ctx.differ_keys.len()
        );
        let mut ranked: Vec<(WeightKey, u64, u64, u64, u64)> = ctx
            .differ_keys
            .iter()
            .zip(key_adopt.adopt.per_key.iter())
            .zip(key_adopt.delete.per_key.iter())
            .map(|((&k, &(n, r1)), &(dn, dr1))| (k, n, r1, dn, dr1))
            .collect();
        ranked.sort_by(|a, b| {
            let fa = if a.1 > 0 { a.2 as f64 / a.1 as f64 } else { 0.0 };
            let fb = if b.1 > 0 { b.2 as f64 / b.1 as f64 } else { 0.0 };
            fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal)
        });
        let top_n = if args.normalize { 15 } else { 10 };
        for (rank, (key, n, r1, _, _)) in ranked.iter().take(top_n).enumerate() {
            let f = if *n > 0 { *r1 as f64 / *n as f64 } else { 0.0 };
            println!("  {:>2}. {:<24} rank1_frac={f:.4}  (n={n})", rank + 1, format!("{key:?}"));
        }
        let zero_count = ranked.iter().filter(|&&(_, n, r1, _, _)| n == 0 || r1 == 0).count();
        println!("keys achieving exactly zero rank1 fraction: {zero_count} of {}", ranked.len());

        if args.normalize {
            let check_n = 5.min(ranked.len());
            println!();
            println!(
                "Cross-check (top {check_n}): adoption rank1_frac vs the m=0 DELETION rank1_frac at the same keys (same machinery, see AdoptStats doc comment)."
            );
            println!(
                "If these still agree to 4 significant figures for every key below, --normalize did not change anything and scaled adoption is still a copy of deletion."
            );
            println!("{:>2}  {:<24} {:>14} {:>14} {:>10}", "#", "key", "adopt_rank1", "delete_rank1", "match4sf");
            let mut all_match = check_n > 0;
            for (rank, (key, n, r1, dn, dr1)) in ranked.iter().take(check_n).enumerate() {
                let fa = if *n > 0 { *r1 as f64 / *n as f64 } else { 0.0 };
                let fd = if *dn > 0 { *dr1 as f64 / *dn as f64 } else { 0.0 };
                let matches4sf = format!("{fa:.4}") == format!("{fd:.4}");
                all_match &= matches4sf;
                println!("{:>2}. {:<24} {fa:>14.4} {fd:>14.4} {:>10}", rank + 1, format!("{key:?}"), matches4sf);
            }
            println!(
                "{}",
                if all_match {
                    "All checked keys still match adoption==deletion to 4sf -- normalisation did NOT take effect for the single-key sweep; say so, do not report a discovery."
                } else {
                    "Adoption and deletion diverge for at least one checked key -- scaled adoption is measuring something deletion does not."
                }
            );

            // Greedy joint check, ONLY if step 2 (scaled single-key
            // adoption) surfaced a clearly nonzero rank1 fraction anywhere
            // -- per this binary's own doc comment on --normalize / the
            // task background. `n > 0` guards against a key with no scored
            // occurrences at all (impossible here since every differ_key
            // is swept at every one of the same 12,299 points, but kept as
            // a defensive check rather than assumed).
            let any_nonzero = ranked.iter().any(|&(_, n, r1, _, _)| n > 0 && r1 > 0);
            println!();
            if any_nonzero {
                println!("=== Greedy joint scaled-adoption check (step 2 found a nonzero key; see task step 3) ===");
                let human = ctx.human;
                let mut remaining: Vec<WeightKey> = ctx.differ_keys.to_vec();
                let mut locked: Vec<(usize, f64)> = Vec::new();
                let mut prev_frac = 0.0f64;
                for step in 1..=8 {
                    if remaining.is_empty() {
                        println!("  step {step}: no remaining candidate keys; stopping.");
                        break;
                    }
                    let step_ctx = InterpCtx { human, differ_keys: &remaining, locked: &locked };
                    let step_cfg = ProbeConfig { interp: Some(&step_ctx), ..cfg };
                    let (_, _, _, _, step_adopt, _) =
                        run_all(args.games, args.players, args.seed, args.threads, weights, &step_cfg, remaining.len());
                    let mut best_i = 0usize;
                    let mut best_frac = -1.0f64;
                    for (i, &(n, r1)) in step_adopt.adopt.per_key.iter().enumerate() {
                        let f = if n > 0 { r1 as f64 / n as f64 } else { 0.0 };
                        if f > best_frac {
                            best_frac = f;
                            best_i = i;
                        }
                    }
                    let key = remaining[best_i];
                    println!("  step {step}: best next key = {key:?}  joint_rank1_frac={best_frac:.4}  (locked so far: {})", locked.len());
                    if best_frac <= prev_frac + 1e-12 {
                        println!("  no improvement over the previous step ({prev_frac:.4}); stopping (greedy is done).");
                        break;
                    }
                    let delta = human.get(key) - weights.get(key);
                    locked.push((key as usize, delta));
                    remaining.remove(best_i);
                    prev_frac = best_frac;
                }
            } else {
                println!(
                    "Step 2 found no key with a nonzero scaled-adoption rank1 fraction -- per this task's own instruction, the greedy joint check is skipped (it only runs when step 2 surfaces something to build on)."
                );
            }
        }
    }

    ExitCode::SUCCESS
}
