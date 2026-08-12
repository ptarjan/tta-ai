//! Baseline human-imitation policy (`docs/HUMAN_MODEL.md`): dataset
//! extraction over real BGO human decisions, and a simple linear scorer
//! fitted to imitate the human's actual choice -- Stage One of the
//! sparring-partner project (see that doc's own top section for the "why").
//!
//! This module is deliberately thin on top of machinery that already exists
//! and is reused, not restated:
//!
//! * [`crate::replay_common::replay_game`] (`record_decisions: true`) --
//!   walks a real BGO journal through the actual engine and stops at every
//!   point a human made a real decision. Already used by `bin/agreement.rs`;
//!   this module adds no new reconstruction logic of its own.
//! * [`crate::bots::weighted::features::features`] -- the SAME raw board
//!   encoding `WeightedBot` is scored with. [`candidate_features`] below
//!   mirrors `WeightedBot::rank_moves`'s own trial-and-encode loop (same
//!   `rival_context` root reuse, same `determinize_current_events` guard
//!   against peeking an unopened event pile) so a caller gets the identical
//!   position-encoding contract real gameplay already trusts, not a parallel
//!   one invented for this project.
//!
//! ## Legality audit (card row/boards/discards public; rival HANDS private)
//!
//! `features::features` never reads a rival's hand CONTENTS -- only public
//! board stats (`rivals::rival_board`, culture/science/resource/food stock,
//! wonder/colony/leader counts -- all physically face-up in the 2015 base
//! game, `rivals.rs`'s own doc comment cites the standing rule) and a
//! rival's hand SIZE (`PlayerState::hand_size_civil`, a count, not an
//! identity). `rivals::rival_context`, which [`candidate_features`] builds
//! once per decision, internally snapshots a `RivalView` (including a
//! rival's `hand_civil` CardList) for OTHER callers (`row::row_pressure`,
//! `cards::rival_hand_potential`) that this module never calls -- confirmed
//! by reading `features()`'s full assignment list: every `Features::set`
//! call reads either the ACTING player's own board/hand, a rival's public
//! board field, or `ctx.rival_culture_rate`/`rival_science_rate`/
//! `rival_strength`/`rival_rates`/`ctx.event_pool` (rate/strength SUMMARIES,
//! never a rival's `RivalView` object itself). This module calls `features()`
//! with `w: None, priced_only: true` -- which also has the effect of
//! *always* skipping `event_scoring_margin` (an expensive Age III-event
//! computation, ~22% of `evaluate`'s own profiled cost per `features.rs`'s
//! doc comment) for every candidate, a deliberate compute-cost
//! simplification, not a privacy decision: it makes that one coordinate a
//! constant 0.0 across this whole dataset rather than a leaked value.
//!
//! No code in this module reads a game's true civil/military deck order or
//! any player's un-revealed hand contents other than the acting player's own
//! (`p.hand_civil`/`p.hand_military`, read inside `features()` for `idx`
//! only -- a player's own hand is never private FROM them).
//!
//! ## Discard taint (`docs/REPLAY.md`'s `discard_solver` section)
//!
//! Every forced military discard in this corpus is an ARBITRARY pick among
//! legal candidates (BGO never names a military card at draw time), never a
//! deduction -- every position downstream of one has a partly fictional
//! simulated military hand. [`ExtractedDecision::discard_tainted`] carries
//! this flag straight off `replay_common::Decision::after_arbitrary_discard`
//! so a consumer can filter or weight it; this module makes no judgement
//! about which is "correct" to train on.

use crate::apply;
use crate::bots::plan;
use crate::bots::weighted::features::{self, Features};
use crate::bots::weighted::rivals;
use crate::bots::weighted::weights::{WeightKey, Weights};
use crate::moves::Move;
use crate::state::GameState;

// --------------------------------------------------------------- encoding

/// Width of every feature vector this module produces -- one slot per
/// [`WeightKey`], the same representation `Features`/`Weights` already use.
pub fn feature_dim() -> usize {
    WeightKey::ALL.len()
}

/// The raw feature vector for every candidate in `moves`, in the SAME order
/// -- mirrors [`crate::bots::weighted::eval::WeightedBot::rank_moves`]'s own
/// trial loop exactly (see this module's top doc comment for the legality
/// argument that makes reusing it safe): one shared `rival_context` root
/// computed off the PRE-move `state`, one shared `determinize_current_events`
/// draw so a `Move::PrepareEvent` candidate cannot leak the true top of an
/// unpeeked pile, then one clone-apply-encode per candidate.
///
/// `moves` is expected to be exactly a real decision point's legal-move list
/// (`replay_common::Decision::legal_moves`) -- deliberately NOT filtered
/// through `filter_resign` the way `WeightedBot::choose`/`rank_moves` are:
/// this module records what the human genuinely faced, not what a live bot
/// would have been allowed to consider.
pub fn candidate_features(state: &GameState, idx: u8, moves: &[Move]) -> Vec<Features> {
    let ctx = rivals::rival_context(state, idx, None, None);
    let mut root = state.clone();
    plan::determinize_current_events(&mut root, &mut plan::plan_rng(state, idx));

    moves
        .iter()
        .map(|&mv| {
            let mut trial = root.clone();
            apply::apply(&mut trial, mv);
            features::features(&trial, idx, Some(&ctx), None, true)
        })
        .collect()
}

// -------------------------------------------------------------- category

/// The move-category buckets `bin/agreement.rs`'s own (private) `Category`
/// already defines for the identical purpose (`docs/AGREEMENT.md`'s move
/// category breakdown) -- restated here, not imported, because that type is
/// private to a different binary crate and this module deliberately avoids
/// editing `bin/agreement.rs`/`docs/AGREEMENT.md` while a second agent is
/// working there concurrently (see this project's own working-procedure
/// notes). Kept intentionally IDENTICAL in shape and mapping so a report
/// built off this module's `category` column reads the same way as
/// `docs/AGREEMENT.md`'s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    TakeCard,
    Build,
    IncreasePopulation,
    LeaderOrWonderStep,
    PoliticalAction,
    AggressionOrWar,
    Pact,
    Tactics,
    Bid,
    EndTurn,
    Other,
}

impl Category {
    pub fn name(self) -> &'static str {
        match self {
            Category::TakeCard => "take_card",
            Category::Build => "build",
            Category::IncreasePopulation => "increase_population",
            Category::LeaderOrWonderStep => "leader_or_wonder_step",
            Category::PoliticalAction => "political_action",
            Category::AggressionOrWar => "aggression_or_war",
            Category::Pact => "pact",
            Category::Tactics => "tactics",
            Category::Bid => "bid",
            Category::EndTurn => "end_turn",
            Category::Other => "other",
        }
    }
}

/// See `bin/agreement.rs::categorize` -- identical exhaustive match, no
/// wildcard arm, so a new `Move` variant is a compile error here too rather
/// than a silent `Other`.
pub fn categorize(mv: Move) -> Category {
    use Move::*;
    match mv {
        Take { .. } => Category::TakeCard,
        Build { .. } | Develop { .. } | Upgrade { .. } => Category::Build,
        Pop | PopFree => Category::IncreasePopulation,
        PlayLeader { .. } | WonderStep { .. } => Category::LeaderOrWonderStep,
        Revolution { .. } | PolPass => Category::PoliticalAction,
        War { .. } | Aggression { .. } => Category::AggressionOrWar,
        OfferPact { .. } => Category::Pact,
        PlayTactic { .. } | CopyTactic { .. } => Category::Tactics,
        Bid { .. } | BidPass => Category::Bid,
        EndTurn => Category::EndTurn,
        // `bin/agreement.rs::categorize_choice` further splits `Choose` by
        // the PRE-move pending's `ChoiceKind` (recovers `Pact` for an
        // accept/refuse). This module does not have that pending snapshot
        // threaded through its own (metadata-light) extraction path, so
        // every `Choose` lands in `Other` here -- a narrower, honestly
        // coarser mapping than `docs/AGREEMENT.md`'s, not a silent gap: see
        // `docs/HUMAN_MODEL.md`'s own note on this.
        Choose { .. }
        | Destroy { .. }
        | PlayAction { .. }
        | CancelPact { .. }
        | PrepareEvent { .. }
        | RemoveLeaderYellow
        | ColumbusColonize { .. }
        | Barbarossa { .. }
        | BachTheater { .. }
        | Defend { .. }
        | DefendDone
        | SendUnit { .. }
        | SendBonus { .. }
        | SendDiscard { .. }
        | SendDone
        | Churchill { .. }
        | Resign
        | TradeFoodAsResource
        | TradeResourceAsFood => Category::Other,
    }
}

// ------------------------------------------------------------- dataset IO

/// One decision point, ready to serialize -- the extraction-side record
/// (holds real [`Features`], produced by [`candidate_features`]).
pub struct ExtractedDecision {
    pub game_id: String,
    pub tier: &'static str,
    pub players: u8,
    pub age: String,
    pub round: u32,
    pub lineno: usize,
    pub category: &'static str,
    pub discard_tainted: bool,
    pub chosen_idx: usize,
    pub candidates: Vec<Features>,
}

/// Sparse `"idx:value"` encoding of one candidate's feature vector, skipping
/// zero coordinates -- `features()`'s own doc comment records that most of
/// the ~139 slots are 0.0 on any given call (only the ones a specific board
/// state actually triggers are set), so this roughly halves file size versus
/// a dense CSV row for free, with no loss (a missing index reads back as
/// 0.0, [`Features`]'s own documented default).
/// The dense counterpart of [`features_to_sparse`] -- every coordinate, in
/// [`WeightKey::ALL`] order, with no zero-skipping. `crate::bots::human::
/// HumanBot` uses this at PLAY time, one candidate at a time, not the sparse
/// text encoding above -- there is no file to keep small on that path, only
/// a `Vec<f64>` [`predict_top1`] can score directly, so the sparse-vs-dense
/// split is purely "which representation does the consumer need," not two
/// competing encodings of the same thing.
pub fn features_to_dense(f: &Features) -> Vec<f64> {
    WeightKey::ALL.iter().map(|&k| f.get(k)).collect()
}

fn features_to_sparse(f: &Features) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (i, &k) in WeightKey::ALL.iter().enumerate() {
        let v = f.get(k);
        if v != 0.0 {
            parts.push(format!("{i}:{v:.4}"));
        }
    }
    parts.join(",")
}

/// One tab-separated line per decision point: `game_id, tier, players, age,
/// round, lineno, category, discard_tainted, chosen_idx`, then a single
/// `;`-separated field of sparse candidate encodings (one candidate's
/// [`features_to_sparse`] per legal move, IN THE SAME ORDER `candidate_idx`
/// indexes into). No header row and no dependency (`Cargo.toml`'s
/// `[dependencies]` stays empty), matching every other TSV this crate emits.
pub fn write_decision(out: &mut impl std::io::Write, r: &ExtractedDecision) -> std::io::Result<()> {
    let cands: Vec<String> = r.candidates.iter().map(features_to_sparse).collect();
    writeln!(
        out,
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        r.game_id,
        r.tier,
        r.players,
        r.age,
        r.round,
        r.lineno,
        r.category,
        r.discard_tainted,
        r.chosen_idx,
        cands.join(";"),
    )
}

/// The read-side counterpart of [`ExtractedDecision`] -- plain data (dense
/// `Vec<f64>` candidates, reconstructed from the sparse encoding), no engine
/// types, so a training binary need not link against `GameState`/`Features`
/// at all.
#[derive(Clone, Debug)]
pub struct ParsedDecision {
    pub game_id: String,
    pub tier: String,
    pub players: u8,
    pub age: String,
    pub round: u32,
    pub lineno: usize,
    pub category: String,
    pub discard_tainted: bool,
    pub chosen_idx: usize,
    pub candidates: Vec<Vec<f64>>,
}

fn parse_sparse(s: &str, dim: usize) -> Result<Vec<f64>, String> {
    let mut v = vec![0.0; dim];
    if s.is_empty() {
        return Ok(v);
    }
    for part in s.split(',') {
        let (idx_s, val_s) = part
            .split_once(':')
            .ok_or_else(|| format!("bad sparse pair {part:?}"))?;
        let idx: usize = idx_s.parse().map_err(|e| format!("bad index {idx_s:?}: {e}"))?;
        if idx >= dim {
            return Err(format!("index {idx} out of range (dim {dim})"));
        }
        let val: f64 = val_s.parse().map_err(|e| format!("bad value {val_s:?}: {e}"))?;
        v[idx] = val;
    }
    Ok(v)
}

/// Parse one [`write_decision`] line back into a [`ParsedDecision`].
pub fn parse_decision(line: &str) -> Result<ParsedDecision, String> {
    let dim = feature_dim();
    let mut f = line.split('\t');
    let mut next = |name: &str| f.next().ok_or_else(|| format!("missing field {name}"));
    let game_id = next("game_id")?.to_string();
    let tier = next("tier")?.to_string();
    let players: u8 = next("players")?.parse().map_err(|e| format!("players: {e}"))?;
    let age = next("age")?.to_string();
    let round: u32 = next("round")?.parse().map_err(|e| format!("round: {e}"))?;
    let lineno: usize = next("lineno")?.parse().map_err(|e| format!("lineno: {e}"))?;
    let category = next("category")?.to_string();
    let discard_tainted: bool = next("discard_tainted")?.parse().map_err(|e| format!("discard_tainted: {e}"))?;
    let chosen_idx: usize = next("chosen_idx")?.parse().map_err(|e| format!("chosen_idx: {e}"))?;
    let cands_field = next("candidates")?;
    let candidates: Vec<Vec<f64>> = if cands_field.is_empty() {
        Vec::new()
    } else {
        cands_field
            .split(';')
            .map(|s| parse_sparse(s, dim))
            .collect::<Result<Vec<_>, String>>()?
    };
    if chosen_idx >= candidates.len() {
        return Err(format!(
            "chosen_idx {chosen_idx} out of range ({} candidates) at {game_id} line {lineno}",
            candidates.len()
        ));
    }
    Ok(ParsedDecision { game_id, tier, players, age, round, lineno, category, discard_tainted, chosen_idx, candidates })
}

/// Deterministic, dependency-free train/held-out split BY GAME (never by
/// decision -- positions from the same game are correlated, so splitting at
/// the decision level would leak the held-out evaluation, `docs/HUMAN_MODEL.md`'s
/// own headline methodology rule). FNV-1a over the game id's bytes, `% 5`:
/// games whose hash lands on 0 are held out (~20%), everything else trains.
/// No `rand` dependency (`Cargo.toml`'s `[dependencies]` is empty) and no
/// seed to manage -- the same game id always lands on the same side, run to
/// run, host to host.
pub fn is_held_out(game_id: &str) -> bool {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in game_id.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash % 5 == 0
}

// -------------------------------------------------------------- training

/// Per-dimension mean/std, fit on the TRAIN split only and applied to both
/// splits -- gradient descent on raw board-count features (some in the
/// single digits, some -- stock counts -- in the dozens) converges far more
/// evenly once every coordinate is on a comparable scale. A coordinate with
/// zero variance in the training data (never fires, or fires at a constant
/// value) is left at `std = 1.0` rather than dividing by zero -- its
/// (constant, now zero-mean) contribution is inert either way.
pub struct Normalizer {
    pub mean: Vec<f64>,
    pub std: Vec<f64>,
}

impl Normalizer {
    pub fn fit(rows: &[ParsedDecision], dim: usize) -> Normalizer {
        let mut sum = vec![0.0f64; dim];
        let mut sumsq = vec![0.0f64; dim];
        let mut n: f64 = 0.0;
        for r in rows {
            for c in &r.candidates {
                n += 1.0;
                for k in 0..dim {
                    sum[k] += c[k];
                    sumsq[k] += c[k] * c[k];
                }
            }
        }
        let mut mean = vec![0.0f64; dim];
        let mut std = vec![1.0f64; dim];
        if n > 0.0 {
            for k in 0..dim {
                mean[k] = sum[k] / n;
                let var = (sumsq[k] / n - mean[k] * mean[k]).max(0.0);
                let s = var.sqrt();
                std[k] = if s > 1e-9 { s } else { 1.0 };
            }
        }
        Normalizer { mean, std }
    }

    pub fn apply(&self, x: &[f64]) -> Vec<f64> {
        x.iter().enumerate().map(|(k, &v)| (v - self.mean[k]) / self.std[k]).collect()
    }
}

fn dot(w: &[f64], x: &[f64]) -> f64 {
    w.iter().zip(x.iter()).map(|(a, b)| a * b).sum()
}

pub struct TrainConfig {
    pub epochs: usize,
    pub learning_rate: f64,
    pub l2: f64,
}

/// Fit a linear scorer by full-batch gradient descent on a multinomial
/// logistic ("softmax over the legal-move list") loss: for each decision,
/// `p_i = softmax(w . x_i)` over its candidates, minimising
/// `-log(p_chosen)` plus L2 shrinkage. This is the standard conditional-logit
/// model for "which of these labelled alternatives did the agent pick" --
/// deliberately NOT a neural net (`docs/HUMAN_MODEL.md`'s brief: keep the
/// baseline simple and honest). `rows`' candidates are expected already
/// normalized (see [`Normalizer`]) -- this function does no scaling of its
/// own.
pub fn train(rows: &[ParsedDecision], dim: usize, cfg: &TrainConfig) -> Vec<f64> {
    let mut w = vec![0.0f64; dim];
    let n = rows.len().max(1) as f64;
    for _epoch in 0..cfg.epochs {
        let mut grad = vec![0.0f64; dim];
        for r in rows {
            if r.candidates.len() < 2 {
                // A single-candidate decision (the human had no real choice)
                // has a constant softmax of 1.0 on its only option --
                // contributes nothing to the gradient (p - target == 0
                // always) but is skipped outright rather than doing the
                // arithmetic to learn that.
                continue;
            }
            let scores: Vec<f64> = r.candidates.iter().map(|c| dot(&w, c)).collect();
            let max = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let exps: Vec<f64> = scores.iter().map(|&s| (s - max).exp()).collect();
            let sum: f64 = exps.iter().sum();
            for (i, cand) in r.candidates.iter().enumerate() {
                let p = exps[i] / sum;
                let target = if i == r.chosen_idx { 1.0 } else { 0.0 };
                let coef = p - target;
                if coef == 0.0 {
                    continue;
                }
                for (k, &v) in cand.iter().enumerate() {
                    if v != 0.0 {
                        grad[k] += coef * v;
                    }
                }
            }
        }
        for k in 0..dim {
            let g = grad[k] / n + cfg.l2 * w[k];
            w[k] -= cfg.learning_rate * g;
        }
    }
    w
}

/// The candidate index the fitted scorer would pick -- first-candidate-wins
/// on an exact tie, matching every other bot in this crate
/// (`WeightedBot::choose`'s own tie-break).
pub fn predict_top1(w: &[f64], candidates: &[Vec<f64>]) -> usize {
    let mut best_i = 0usize;
    let mut best_v = f64::NEG_INFINITY;
    for (i, c) in candidates.iter().enumerate() {
        let s = dot(w, c);
        if s > best_v {
            best_v = s;
            best_i = i;
        }
    }
    best_i
}

/// Every candidate index, best human-ranked score first -- [`predict_top1`]
/// keeps only the winner; a move-ordering PRIOR
/// (`bots::human::choose_with_search`'s root shortlist) needs the whole
/// order, to keep more than one candidate before handing the survivors to a
/// real state-value search. Ties keep candidate-list order (a stable sort),
/// matching [`predict_top1`]'s own first-candidate-wins tie-break.
pub fn rank(w: &[f64], candidates: &[Vec<f64>]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..candidates.len()).collect();
    order.sort_by(|&a, &b| dot(w, &candidates[b]).partial_cmp(&dot(w, &candidates[a])).unwrap());
    order
}

/// Convert a vector [`train`] fit on NORMALIZED features (every candidate
/// having gone through `norm.apply`) into an equivalent vector that ranks
/// RAW, un-normalized features the same way [`predict_top1`] would have
/// ranked the normalized ones -- what [`crate::bots::human::HumanBot`] needs,
/// since [`candidate_features`] hands it raw [`Features`], never normalized
/// ones (there is no train-split statistic to normalize against at play
/// time, and there must not be one: normalizing against a live game's own
/// candidates would be a second, ad hoc feature scale that was never fit
/// against anything).
///
/// Only valid for RANKING (argmax over one decision's candidates), not for
/// reading the score's absolute value: distributing the dot product,
/// `w . x_norm = w . ((x - mean) / std) = sum_k (w_k / std_k) * x_k -
/// sum_k (w_k * mean_k / std_k)`, splits it into a per-coordinate
/// COEFFICIENT (`w_k / std_k`, computed here) and a per-decision additive
/// CONSTANT (the second sum) that is IDENTICAL across every candidate of the
/// same decision (it depends only on `w`/`mean`/`std`, never on the
/// candidate `x`). A constant added to every candidate's score cannot move
/// the argmax, so dropping it here is exact for ranking and would be wrong
/// for anything that reads the score itself.
pub fn denormalize_for_ranking(w: &[f64], norm: &Normalizer) -> Vec<f64> {
    w.iter().zip(norm.std.iter()).map(|(&wk, &sk)| wk / sk).collect()
}

// ------------------------------------------------- persisted play weights

/// Pack a [`train`]-fitted vector (dim [`feature_dim`], in [`WeightKey::ALL`]
/// order) into a [`Weights`] -- the same `WeightKey`-indexed container
/// `WeightedBot`'s champion vector already uses, reused here rather than
/// inventing a second array-indexed-by-enum shape. Every key is written
/// (never left at [`Weights::defaults`]'s hand-tuned champion default), so
/// the result depends only on `v`.
///
/// # Errors
/// If `v.len() != feature_dim()`.
pub fn vector_to_weights(v: &[f64]) -> Result<Weights, String> {
    if v.len() != WeightKey::ALL.len() {
        return Err(format!("expected {} weights, got {}", WeightKey::ALL.len(), v.len()));
    }
    let mut w = Weights::default();
    for (i, &k) in WeightKey::ALL.iter().enumerate() {
        w.set(k, v[i]);
    }
    Ok(w)
}

/// The inverse of [`vector_to_weights`] -- reads every [`WeightKey`] back out
/// in [`WeightKey::ALL`] order, the order [`train`]/[`predict_top1`] expect.
pub fn vector_from_weights(w: &Weights) -> Vec<f64> {
    WeightKey::ALL.iter().map(|&k| w.get(k)).collect()
}

/// Render a fitted [`Weights`] vector as a sorted `{"name": value}` JSON
/// object -- the same dependency-free-JSON, sorted-key, one-space-indent
/// convention `bots::weighted::eval::weights_json` already writes a champion
/// vector in (reused, not reinvented), but WITHOUT that function's `"gen"`/
/// `"weights"`-wrapper bookkeeping (this vector has none) and, critically,
/// without ever routing through `bots::weighted::eval::parse_weights`'s
/// `dominance_repair` on the way back: that repair enforces gameplay-EVALUATOR
/// monotonicity invariants (`hi >= lo`, "a benefit weight is never negative",
/// early/late phase pairs net non-negative) that a vector fit purely to
/// imitate which move a human PICKED was never trained to satisfy and has no
/// reason to. Routing this file through `parse_weights` would silently
/// rewrite whichever fitted coordinates violate those invariants -- exactly
/// the round-trip break [`parse_weights_text`] exists to avoid (see
/// `a_weights_vector_round_trips_through_the_human_weights_text_encoding`
/// below).
pub fn weights_to_text(w: &Weights) -> String {
    let mut names: Vec<&'static str> = WeightKey::ALL.iter().map(|k| k.name()).collect();
    names.sort_unstable();
    let mut out = String::from("{\n");
    for (i, name) in names.iter().enumerate() {
        let key = WeightKey::by_name(name).expect("name came from WeightKey::ALL");
        let sep = if i + 1 == names.len() { "\n" } else { ",\n" };
        out.push_str(&format!("  \"{name}\": {}{sep}", w.get(key)));
    }
    out.push('}');
    out
}

/// Parse [`weights_to_text`]'s output back into a [`Weights`] -- see that
/// function's doc comment for why this is a dedicated read path rather than
/// `bots::weighted::eval::parse_weights`. A key absent from the JSON reads
/// back as 0.0 (matching [`train`]'s own zero-initialized starting vector --
/// a coordinate the fit never moved really is zero here, not
/// `WeightKey::default_weight`'s unrelated hand-tuned champion default). An
/// unknown key name is a hard error, same as `parse_weights`.
pub fn parse_weights_text(text: &str) -> Result<Weights, String> {
    let doc = crate::fixtures::parse_json(text).map_err(|e| format!("{e:?}"))?;
    let fields = match &doc {
        crate::fixtures::Json::Obj(fields) => fields,
        _ => return Err("human weights JSON is not an object".to_string()),
    };
    let mut w = Weights::default();
    for &k in WeightKey::ALL {
        w.set(k, 0.0);
    }
    for (name, value) in fields {
        // A RETIRED name is dropped, not rejected -- exactly as
        // `eval::load_weights` does. This loader is deliberately separate
        // from that one (see `weights_to_text`), but "retired" is a property
        // of the weight basis itself, not of one reader: a file written
        // before a key was retired must stay loadable by BOTH, or retiring a
        // key silently breaks every frozen pool member and anchor on disk
        // while the champion files keep working.
        if crate::bots::weighted::weights::RETIRED_KEYS.contains(&name.as_str()) {
            continue;
        }
        let key = WeightKey::by_name(name).ok_or_else(|| format!("unknown weight {name:?}"))?;
        let v = value.as_f64().ok_or_else(|| format!("weight {name:?} is not a number"))?;
        if !v.is_finite() {
            return Err(format!("weight {name:?} is not finite"));
        }
        w.set(key, v);
    }
    Ok(w)
}

/// Read a persisted human-imitation weight file from disk. See
/// [`weights_to_text`] for why this is not `bots::weighted::eval::
/// load_weights`.
pub fn load_weights(path: &std::path::Path) -> Result<Weights, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_weights_text(&text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Write a persisted human-imitation weight file where [`load_weights`] will
/// read it back. `humantrain`'s only I/O side effect, and the fix for GAP 1
/// (`docs/HUMAN_MODEL.md`): the fitted vector used to be printed as an
/// accuracy number and thrown away.
pub fn save_weights(path: &std::path::Path, w: &Weights) -> Result<(), String> {
    for &k in WeightKey::ALL {
        if !w.get(k).is_finite() {
            return Err(format!("weight {:?} is not finite", k.name()));
        }
    }
    std::fs::write(path, weights_to_text(w)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A toy two-candidate dataset where ONE feature coordinate perfectly
    /// separates the chosen move from the rejected one (chosen always has
    /// coordinate 0 set to 1.0, rejected always 0.0) -- gradient descent must
    /// learn a positive weight there and reach perfect top-1 accuracy, the
    /// basic "this optimizer actually fits something" smoke test.
    #[test]
    fn training_perfectly_separable_toy_data_reaches_100pct_accuracy() {
        let dim = 3;
        let mut rows = Vec::new();
        for i in 0..20 {
            let chosen = vec![1.0, 0.0, (i % 2) as f64];
            let rejected = vec![0.0, 1.0, ((i + 1) % 2) as f64];
            rows.push(ParsedDecision {
                game_id: format!("g{i}"),
                tier: "Emperor".to_string(),
                players: 2,
                age: "A".to_string(),
                round: 1,
                lineno: i,
                category: "build".to_string(),
                discard_tainted: false,
                chosen_idx: 0,
                candidates: vec![chosen, rejected],
            });
        }
        let cfg = TrainConfig { epochs: 200, learning_rate: 0.5, l2: 0.0 };
        let w = train(&rows, dim, &cfg);
        let mut correct = 0;
        for r in &rows {
            if predict_top1(&w, &r.candidates) == r.chosen_idx {
                correct += 1;
            }
        }
        assert_eq!(correct, rows.len(), "must fit a perfectly separable toy dataset exactly, weights={w:?}");
    }

    /// A decision with only one legal candidate must never move the
    /// gradient (nothing to learn -- the human had no alternative) and must
    /// always be predicted correctly regardless of the fitted weights.
    #[test]
    fn a_single_candidate_decision_is_skipped_by_training_and_always_predicted_correctly() {
        let dim = 2;
        let rows = vec![ParsedDecision {
            game_id: "g0".to_string(),
            tier: "Emperor".to_string(),
            players: 2,
            age: "A".to_string(),
            round: 1,
            lineno: 0,
            category: "end_turn".to_string(),
            discard_tainted: false,
            chosen_idx: 0,
            candidates: vec![vec![3.0, -1.0]],
        }];
        let cfg = TrainConfig { epochs: 10, learning_rate: 0.1, l2: 0.0 };
        let w = train(&rows, dim, &cfg);
        assert_eq!(w, vec![0.0, 0.0], "a single-candidate decision must contribute zero gradient");
        assert_eq!(predict_top1(&w, &rows[0].candidates), 0);
    }

    /// `write_decision`/`parse_decision` round-trip every field, including
    /// zero feature coordinates (which the sparse encoding must reconstruct
    /// as exactly 0.0, not drop as missing data).
    #[test]
    fn a_decision_round_trips_through_the_tsv_encoding() {
        use crate::game as G;
        let state = G::new_game(2, 7);
        let cands = candidate_features(&state, 0, crate::legal::legal_moves(&state).as_slice());
        let r = ExtractedDecision {
            game_id: "7523818".to_string(),
            tier: "Emperor",
            players: 2,
            age: "A".to_string(),
            round: 1,
            lineno: 42,
            category: "take_card",
            discard_tainted: true,
            chosen_idx: 0,
            candidates: cands,
        };
        let mut buf = Vec::new();
        write_decision(&mut buf, &r).unwrap();
        let line = String::from_utf8(buf).unwrap();
        let parsed = parse_decision(line.trim_end()).unwrap();
        assert_eq!(parsed.game_id, "7523818");
        assert_eq!(parsed.tier, "Emperor");
        assert_eq!(parsed.players, 2);
        assert_eq!(parsed.round, 1);
        assert_eq!(parsed.lineno, 42);
        assert_eq!(parsed.category, "take_card");
        assert!(parsed.discard_tainted);
        assert_eq!(parsed.chosen_idx, 0);
        assert_eq!(parsed.candidates.len(), r.candidates.len());
        for (i, f) in r.candidates.iter().enumerate() {
            for (k, &key) in WeightKey::ALL.iter().enumerate() {
                assert_eq!(parsed.candidates[i][k], f.get(key), "candidate {i} coord {k}");
            }
        }
    }

    /// `is_held_out` is a pure function of the game id -- same id, same
    /// answer, every call -- and splits a real spread of ids into both
    /// buckets (not a degenerate always-true/always-false hash).
    #[test]
    fn is_held_out_is_deterministic_and_splits_both_ways_over_real_looking_ids() {
        let ids: Vec<String> = (7_520_000..7_520_200).map(|i| i.to_string()).collect();
        let first_pass: Vec<bool> = ids.iter().map(|id| is_held_out(id)).collect();
        let second_pass: Vec<bool> = ids.iter().map(|id| is_held_out(id)).collect();
        assert_eq!(first_pass, second_pass);
        assert!(first_pass.iter().any(|&b| b), "expected at least one held-out id in 200");
        assert!(first_pass.iter().any(|&b| !b), "expected at least one training id in 200");
    }

    /// [`Normalizer::fit`] centers every coordinate to (approximately) zero
    /// mean on the data it was fit on, and never divides by a zero std (a
    /// constant coordinate must not become `NaN`/`inf`).
    #[test]
    fn normalizer_centers_training_data_and_never_divides_by_zero_std() {
        let dim = 2;
        let rows = vec![
            ParsedDecision {
                game_id: "g0".to_string(),
                tier: "Emperor".to_string(),
                players: 2,
                age: "A".to_string(),
                round: 1,
                lineno: 0,
                category: "build".to_string(),
                discard_tainted: false,
                chosen_idx: 0,
                candidates: vec![vec![10.0, 5.0], vec![20.0, 5.0]],
            },
            ParsedDecision {
                game_id: "g1".to_string(),
                tier: "Emperor".to_string(),
                players: 2,
                age: "A".to_string(),
                round: 1,
                lineno: 0,
                category: "build".to_string(),
                discard_tainted: false,
                chosen_idx: 0,
                candidates: vec![vec![30.0, 5.0], vec![0.0, 5.0]],
            },
        ];
        let norm = Normalizer::fit(&rows, dim);
        assert_eq!(norm.std[1], 1.0, "a constant coordinate (always 5.0) must not divide by zero");
        let mut total = vec![0.0; dim];
        let mut n = 0.0;
        for r in &rows {
            for c in &r.candidates {
                let normed = norm.apply(c);
                for k in 0..dim {
                    total[k] += normed[k];
                }
                n += 1.0;
            }
        }
        for k in 0..dim {
            assert!((total[k] / n).abs() < 1e-9, "coord {k} mean should be ~0 after normalizing, got {}", total[k] / n);
        }
    }

    /// [`denormalize_for_ranking`]'s whole point: scoring RAW candidates with
    /// the denormalized vector must rank them identically to scoring
    /// NORMALIZED candidates with the original vector -- the property that
    /// makes it safe for `HumanBot` to skip normalizing at play time.
    #[test]
    fn denormalize_for_ranking_preserves_the_argmax_over_raw_candidates() {
        let dim = 2;
        let rows = vec![ParsedDecision {
            game_id: "g0".to_string(),
            tier: "Emperor".to_string(),
            players: 2,
            age: "A".to_string(),
            round: 1,
            lineno: 0,
            category: "build".to_string(),
            discard_tainted: false,
            chosen_idx: 0,
            candidates: vec![vec![10.0, 100.0], vec![20.0, 100.0], vec![5.0, 100.0]],
        }];
        let norm = Normalizer::fit(&rows, dim);
        let w = vec![0.3, -0.2]; // an arbitrary fitted vector, in NORMALIZED space
        let normalized_candidates: Vec<Vec<f64>> =
            rows[0].candidates.iter().map(|c| norm.apply(c)).collect();
        let expect_best = predict_top1(&w, &normalized_candidates);

        let raw_w = denormalize_for_ranking(&w, &norm);
        let got_best = predict_top1(&raw_w, &rows[0].candidates);
        assert_eq!(got_best, expect_best);
    }

    /// A fitted vector round-trips through [`weights_to_text`]/
    /// [`parse_weights_text`] exactly -- the property [`vector_to_weights`]'s
    /// doc comment claims, and the property that would break if this module
    /// ever routed through `bots::weighted::eval::parse_weights`'s
    /// `dominance_repair` instead (deliberately not exercised here for that
    /// reason).
    #[test]
    fn a_weights_vector_round_trips_through_the_human_weights_text_encoding() {
        let dim = feature_dim();
        // A handful of deliberately "not dominance-clean" values -- some
        // negative where a champion vector would never be, so this test
        // would catch a regression to the champion loader immediately.
        let v: Vec<f64> = (0..dim).map(|i| (i as f64) * 0.37 - 12.5).collect();
        let w = vector_to_weights(&v).unwrap();
        let text = weights_to_text(&w);
        let parsed = parse_weights_text(&text).unwrap();
        assert_eq!(vector_from_weights(&parsed), v);
    }

    /// [`vector_to_weights`] rejects a vector of the wrong width rather than
    /// silently truncating or padding it -- a caller that mismatches
    /// `feature_dim()` has a real bug worth failing loudly on.
    #[test]
    fn vector_to_weights_rejects_the_wrong_width() {
        assert!(vector_to_weights(&[0.0; 3]).is_err());
    }

    /// [`parse_weights_text`] rejects a key name it does not recognise --
    /// the same "unknown key is a hard error" contract `parse_weights`
    /// documents, restated here since this is a separate parser.
    #[test]
    fn parse_weights_text_rejects_an_unknown_key() {
        assert!(parse_weights_text(r#"{"not_a_real_weight": 1.0}"#).is_err());
    }

    /// [`load_weights`]/[`save_weights`] round-trip through an actual file on
    /// disk, not just the in-memory text functions -- the path `humantrain`
    /// and `HumanBot` actually use.
    #[test]
    fn save_weights_then_load_weights_round_trips_through_a_real_file() {
        let dim = feature_dim();
        let v: Vec<f64> = (0..dim).map(|i| (i as f64 - 5.0) * 0.11).collect();
        let w = vector_to_weights(&v).unwrap();
        let dir = std::env::temp_dir().join(format!("human_policy_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("human_weights_roundtrip.json");
        save_weights(&path, &w).unwrap();
        let loaded = load_weights(&path).unwrap();
        assert_eq!(vector_from_weights(&loaded), v);
        std::fs::remove_file(&path).ok();
    }

    /// [`features_to_dense`] reads every coordinate in [`WeightKey::ALL`]
    /// order with no zero-skipping -- the exact contract [`HumanBot`]
    /// (`crate::bots::human`) depends on to hand [`predict_top1`] a `Vec<f64>`
    /// it can score without going through the sparse text encoding at all.
    #[test]
    fn features_to_dense_reads_every_coordinate_in_weight_key_order() {
        use crate::game as G;
        let state = G::new_game(2, 11);
        let cands = candidate_features(&state, 0, crate::legal::legal_moves(&state).as_slice());
        for f in &cands {
            let dense = features_to_dense(f);
            assert_eq!(dense.len(), feature_dim());
            for (k, &key) in WeightKey::ALL.iter().enumerate() {
                assert_eq!(dense[k], f.get(key));
            }
        }
    }
}
