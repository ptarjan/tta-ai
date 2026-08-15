//! `agreefit` -- does the champion's weakness live in its WEIGHTS or its
//! FEATURES? (project brief: fit [`tta::bots::weighted::eval::WeightedBot`]'s
//! ~140 weights DIRECTLY to strong-human move choices by supervised
//! multinomial softmax, then measure top-1 agreement on HELD-OUT games. If
//! agreement climbs a lot over the champion's hill-climbed 21.4%
//! (`docs/CHAMPION_VS_HUMANS.md`), the feature basis can already express
//! human-like play and the league just never found this vector -- a WEIGHTS
//! verdict. If it barely moves, the basis is structurally blind to whatever
//! humans are responding to -- a FEATURES verdict.
//!
//! # Reused, not restated
//!
//! * [`tta::replay_common::replay_game`] (`record_decisions: true`) -- the
//!   SAME decision-point walker `bin/agreement.rs` uses. This file adds no
//!   second replay walker.
//! * [`tta::bots::weighted::eval::candidate_features`]/[`linear_features`]
//!   -- the SAME feature-extraction code path [`WeightedBot::rank_moves`]/
//!   `choose` trial-and-evaluate over, reused here instead of a second copy
//!   (see that module's own doc comment for the one documented gap: ten
//!   identity-aware coordinates are bilinear in the weight vector in the
//!   real `evaluate`, so they are frozen at the CHAMPION's numbers for
//!   feature extraction here -- their own outer gate weight is still fully
//!   fit).
//! * [`tta::replay_common::categorize`] -- the SAME move-category labelling
//!   `bin/agreement.rs` reports by, moved to the library so this binary and
//!   that one never carry two copies.
//!
//! # Method
//!
//! 1. Select up to `TRAIN_GAMES` + `HOLDOUT_GAMES` Warlord/Emperor games
//!    with a journal on disk, split by GAME (never trained and measured on
//!    the same game) via a cheap multiplicative hash of the numeric game id
//!    -- deterministic, and decorrelated from `index.tsv`'s own
//!    (chronological) order.
//! 2. Extraction pass: replay every selected game once, and for every
//!    recorded decision point, cache `(game_id, lineno, category, age,
//!    human_index, candidate feature vectors)` to a compact binary file
//!    (`--cache-dir`) -- expensive (replays + trial-applies every
//!    candidate), done once, never repeated across epochs.
//! 3. Read the cache fully into memory and train a linear score
//!    `w . f_scaled` by streaming multinomial softmax cross-entropy
//!    (Adam, standardized features, a modest L2), one decision (one softmax
//!    over its own candidate list) at a time, for a fixed, small number of
//!    epochs -- ONE fit, not tuned to a target number (project brief: "one
//!    honest fit... is the deliverable").
//! 4. Report top-1 agreement of the CHAMPION, FITTED, and all-ZERO weight
//!    vectors on the held-out split (and the fitted vector's OWN train-set
//!    agreement, so overfitting is visible), broken down by category and
//!    age, plus a blind-spot scan naming concrete training-set decisions the
//!    fit could not rank well even with every chance to.
//! 5. Write the fitted vector to `<out-dir>/fitted_{2,3,4}p.json` in the
//!    same JSON shape `analysis/frozen/gauntlet/*.json` already uses (one
//!    shared vector duplicated three times -- see `main`'s own doc comment
//!    on why this run fits a single pooled vector across all player counts
//!    rather than three separately-starved ones).
//!
//! ```text
//! cargo run --profile difftest --bin agreefit -- \
//!     sources/bgo/index.tsv /tmp/bgojournals/journals \
//!     analysis/frozen/gauntlet/champion_2p_gen1454_140key_2026-08-06.json \
//!     analysis/frozen/gauntlet/champion_3p_gen1384_140key_2026-08-06.json \
//!     analysis/frozen/gauntlet/champion_4p_gen448_140key_2026-08-06.json \
//!     /tmp/agreefit_cache /tmp/agreefit_out
//! ```

use std::collections::HashMap;
use std::env;
use std::fs;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tta::bots::weighted::eval::{self, load_weights, save_weights};
use tta::bots::weighted::weights::{WeightKey, Weights};
use tta::corpus::{self, GameMeta, Tier};
use tta::replay_common::{build_card_index, categorize, replay_game, Category};
use tta::rng::PyRandom;

const TRAIN_GAMES: usize = 250;
const HOLDOUT_GAMES: usize = 120;
const EPOCHS: u32 = 6;
const LEARNING_RATE: f64 = 0.05;
const L2: f64 = 3e-4;
const ADAM_BETA1: f64 = 0.9;
const ADAM_BETA2: f64 = 0.999;
const ADAM_EPS: f64 = 1e-8;
const TRAIN_SEED: i128 = 20260808;

// ------------------------------------------------------------ game selection

/// A cheap multiplicative hash (Knuth's constant, truncated to `u64`) of a
/// game id -- used only to decorrelate the train/held-out split from
/// `index.tsv`'s own chronological order without pulling in a `rand` crate
/// this crate's empty `[dependencies]` deliberately has none of.
fn hash_id(id: &str) -> u64 {
    let n: u64 = id.parse().unwrap_or(0);
    n.wrapping_mul(2_654_435_761)
}

/// Warlord/Emperor games with a journal file on disk, split by GAME into a
/// train set and a disjoint held-out set -- see this module's own doc
/// comment on why the split is hashed rather than sliced off `index.tsv`'s
/// own order. [`GameMeta`] carries no `Clone`, so this borrows from `games`
/// rather than copying.
fn select_games<'a>(games: &'a [GameMeta], journals_dir: &str) -> (Vec<&'a GameMeta>, Vec<&'a GameMeta>) {
    let mut strong: Vec<&GameMeta> = games
        .iter()
        .filter(|g| matches!(g.tier, Tier::Warlord | Tier::Emperor))
        .filter(|g| Path::new(&format!("{journals_dir}/{}.tsv", g.id)).exists())
        .collect();
    strong.sort_by_key(|g| hash_id(&g.id));

    let train: Vec<&GameMeta> = strong.iter().take(TRAIN_GAMES).copied().collect();
    let holdout: Vec<&GameMeta> = strong.iter().skip(TRAIN_GAMES).take(HOLDOUT_GAMES).copied().collect();
    (train, holdout)
}

// ------------------------------------------------------------- cache format
//
// One binary file per split: a 4-byte magic, a `u32` feature width, then one
// record per recorded decision:
//   u32 game_id, u32 lineno, u8 category, u8 age, u8 n_candidates,
//   u8 human_index, then `n_candidates * nfeat` little-endian `f32`s (one
//   candidate's `linear_features` vector after another, `human_index`'s own
//   entry included in its own slot like every other candidate).

const CACHE_MAGIC: &[u8; 4] = b"AGF1";

fn category_code(c: Category) -> u8 {
    match c {
        Category::TakeCard => 0,
        Category::Build => 1,
        Category::IncreasePopulation => 2,
        Category::LeaderOrWonderStep => 3,
        Category::PoliticalAction => 4,
        Category::AggressionOrWar => 5,
        Category::Pact => 6,
        Category::Tactics => 7,
        Category::Bid => 8,
        Category::EndTurn => 9,
        Category::Other => 10,
    }
}

const CATEGORY_NAMES: [&str; 11] =
    ["take_card", "build", "increase_population", "leader_or_wonder_step", "political_action",
     "aggression_or_war", "pact", "tactics", "bid", "end_turn", "other"];

const AGE_NAMES: [&str; 5] = ["A", "I", "II", "III", "IV"];

/// Whether `category` is one of `docs/CHAMPION_VS_HUMANS.md`'s four named
/// weak categories -- `aggression_or_war` there is the corpus-wide finding's
/// own name for what [`categorize`] calls `Category::AggressionOrWar`.
fn is_weak_category(code: u8) -> bool {
    matches!(code, 0 | 2 | 3 | 5) // take_card, increase_population, leader_or_wonder_step, aggression_or_war
}

struct CacheWriter {
    w: BufWriter<File>,
    nfeat: usize,
}

impl CacheWriter {
    fn create(path: &Path, nfeat: usize) -> Result<Self, String> {
        let mut w = BufWriter::new(File::create(path).map_err(|e| format!("{}: {e}", path.display()))?);
        w.write_all(CACHE_MAGIC).map_err(|e| e.to_string())?;
        w.write_all(&(nfeat as u32).to_le_bytes()).map_err(|e| e.to_string())?;
        Ok(CacheWriter { w, nfeat })
    }

    fn write_decision(
        &mut self,
        game_id: u32,
        lineno: u32,
        category: u8,
        age: u8,
        human_index: u8,
        candidates: &[Vec<f64>],
    ) -> Result<(), String> {
        let n = candidates.len();
        if n > 255 {
            return Err(format!("game {game_id} line {lineno}: {n} candidates exceeds u8 cap"));
        }
        self.w.write_all(&game_id.to_le_bytes()).map_err(|e| e.to_string())?;
        self.w.write_all(&lineno.to_le_bytes()).map_err(|e| e.to_string())?;
        self.w.write_all(&[category, age, n as u8, human_index]).map_err(|e| e.to_string())?;
        for f in candidates {
            debug_assert_eq!(f.len(), self.nfeat);
            for &v in f {
                self.w.write_all(&(v as f32).to_le_bytes()).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// One cached decision point, read back into memory -- `candidates` is
/// `n_candidates * nfeat` `f32`s, flattened, one candidate after another
/// (`Self::candidate`'s own job is slicing that back apart).
struct CachedDecision {
    game_id: u32,
    lineno: u32,
    category: u8,
    age: u8,
    human_index: usize,
    n: usize,
    candidates: Vec<f32>,
}

impl CachedDecision {
    fn candidate(&self, i: usize, nfeat: usize) -> &[f32] {
        &self.candidates[i * nfeat..(i + 1) * nfeat]
    }
}

fn read_cache(path: &Path) -> Result<(usize, Vec<CachedDecision>), String> {
    let mut r = BufReader::new(File::open(path).map_err(|e| format!("{}: {e}", path.display()))?);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != CACHE_MAGIC {
        return Err(format!("{}: bad cache magic", path.display()));
    }
    let mut nfeat_buf = [0u8; 4];
    r.read_exact(&mut nfeat_buf).map_err(|e| e.to_string())?;
    let nfeat = u32::from_le_bytes(nfeat_buf) as usize;

    let mut out = Vec::new();
    loop {
        let mut head = [0u8; 12];
        match r.read_exact(&mut head) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        }
        let game_id = u32::from_le_bytes(head[0..4].try_into().unwrap());
        let lineno = u32::from_le_bytes(head[4..8].try_into().unwrap());
        let category = head[8];
        let age = head[9];
        let n = head[10] as usize;
        let human_index = head[11] as usize;
        let mut raw = vec![0u8; n * nfeat * 4];
        r.read_exact(&mut raw).map_err(|e| e.to_string())?;
        let mut candidates = Vec::with_capacity(n * nfeat);
        for chunk in raw.chunks_exact(4) {
            candidates.push(f32::from_le_bytes(chunk.try_into().unwrap()));
        }
        out.push(CachedDecision { game_id, lineno, category, age, human_index, n, candidates });
    }
    Ok((nfeat, out))
}

// --------------------------------------------------------------- extraction

fn extract_split(
    games: &[&GameMeta],
    journals_dir: &str,
    card_index: &HashMap<&'static str, tta::CardId>,
    champ: &HashMap<u8, Weights>,
    cache_path: &Path,
) -> Result<(usize, usize), String> {
    let nfeat = WeightKey::ALL.len();
    let mut writer = CacheWriter::create(cache_path, nfeat)?;
    let mut n_decisions = 0usize;
    let mut n_games_ok = 0usize;

    for meta in games {
        let path = format!("{journals_dir}/{}.tsv", meta.id);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{}: skipping, no journal ({e})", meta.id);
                continue;
            }
        };
        let freeze = champ
            .get(&meta.players)
            .ok_or_else(|| format!("no champion weights loaded for {}p", meta.players))?;
        let result = replay_game(meta, &text, card_index, true);
        n_games_ok += 1;
        let gid: u32 = meta.id.parse().unwrap_or(0);

        for d in &result.decisions {
            let candidates = eval::candidate_features(&d.state, &d.legal_moves, false, freeze);
            let Some(human_index) = candidates.iter().position(|&(mv, _)| mv == d.human_move) else {
                // `filter_resign` can only ever drop `Move::Resign` -- the
                // human's own recorded move is always a member of
                // `d.legal_moves` by construction (`try_apply` records only
                // after its own legality check passes), so this is only
                // reachable if the human's real move WAS `Resign` and a
                // live alternative existed too. Skip rather than panic --
                // `docs/AGREEMENT.md`'s own `human_rank: uncounted` posture.
                continue;
            };
            let category = category_code(categorize(d.state.pending.top(), d.human_move));
            let age = d.state.age_civil as u8;
            let feats: Vec<Vec<f64>> = candidates.into_iter().map(|(_, f)| f).collect();
            writer.write_decision(gid, d.lineno as u32, category, age, human_index as u8, &feats)?;
            n_decisions += 1;
        }
    }
    writer.w.flush().map_err(|e| e.to_string())?;
    Ok((n_games_ok, n_decisions))
}

// ----------------------------------------------------------------- training

fn shuffle(order: &mut [usize], rng: &mut PyRandom) {
    for i in (1..order.len()).rev() {
        let j = (rng.random() * (i as f64 + 1.0)) as usize;
        let j = j.min(i);
        order.swap(i, j);
    }
}

/// Per-feature mean/std over every candidate in `train` -- standardizing
/// keeps Adam's step size sane across features whose raw magnitude ranges
/// from small counts to `hand_potential`'s tens-of-points scale. Centering
/// is safe for the softmax loss below despite `w . f` losing its face-value
/// meaning: every candidate at the SAME decision gets the SAME per-feature
/// mean subtracted, so it contributes an identical additive constant to
/// every candidate's score at that decision and cancels exactly in the
/// softmax (and in an argmax over that decision) -- never restated when
/// converting the fitted vector back to raw-feature units afterward.
fn standardize(train: &[CachedDecision], nfeat: usize) -> (Vec<f64>, Vec<f64>) {
    let mut sum = vec![0.0f64; nfeat];
    let mut sumsq = vec![0.0f64; nfeat];
    let mut count = 0u64;
    for d in train {
        for i in 0..d.n {
            let f = d.candidate(i, nfeat);
            for j in 0..nfeat {
                let v = f[j] as f64;
                sum[j] += v;
                sumsq[j] += v * v;
            }
            count += 1;
        }
    }
    let mut mean = vec![0.0f64; nfeat];
    let mut std = vec![1.0f64; nfeat];
    if count == 0 {
        return (mean, std);
    }
    for j in 0..nfeat {
        let m = sum[j] / count as f64;
        mean[j] = m;
        let var = (sumsq[j] / count as f64 - m * m).max(0.0);
        let s = var.sqrt();
        if s > 1e-9 {
            std[j] = s;
        }
    }
    (mean, std)
}

/// Streaming multinomial softmax cross-entropy, Adam, over `train` -- one
/// decision (one softmax over its own candidate list) per step, `EPOCHS`
/// passes with the order reshuffled each time. Decisions with fewer than
/// two candidates carry no gradient (a one-candidate softmax is always
/// exactly right) and are skipped here, though they stay in the cache for
/// agreement measurement (which counts every recorded decision, matching
/// `bin/agreement.rs`'s own corpus-wide methodology).
fn train_softmax(train: &[CachedDecision], nfeat: usize, mean: &[f64], std: &[f64]) -> Vec<f64> {
    let mut w = vec![0.0f64; nfeat];
    let mut adam_m = vec![0.0f64; nfeat];
    let mut adam_v = vec![0.0f64; nfeat];
    let mut t_step: i32 = 0;

    let mut order: Vec<usize> = (0..train.len()).filter(|&i| train[i].n >= 2).collect();
    let mut rng = PyRandom::new(TRAIN_SEED);

    for epoch in 0..EPOCHS {
        shuffle(&mut order, &mut rng);
        let mut total_loss = 0.0f64;
        let mut n_steps = 0u64;

        for &idx in &order {
            let d = &train[idx];
            let mut scores = vec![0.0f64; d.n];
            // Same reasoning as bots/neural/train.rs's own needless_range_loop allows:
            // this is dense training-math indexing several parallel arrays (`scores`,
            // `p`, `grad`, `f = d.candidate(i, nfeat)`) together; a hand-zipped
            // rewrite risks silently misaligning gradient math with no compiler
            // signal, which is worse than the style lint it would silence.
            #[allow(clippy::needless_range_loop)]
            for i in 0..d.n {
                let f = d.candidate(i, nfeat);
                let mut s = 0.0;
                for j in 0..nfeat {
                    s += w[j] * ((f[j] as f64 - mean[j]) / std[j]);
                }
                scores[i] = s;
            }
            let m = scores.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let mut z = 0.0;
            let mut p = vec![0.0f64; d.n];
            for i in 0..d.n {
                p[i] = (scores[i] - m).exp();
                z += p[i];
            }
            for pi in &mut p {
                *pi /= z;
            }
            total_loss += -(scores[d.human_index] - m) + z.ln();
            n_steps += 1;

            let mut grad = vec![0.0f64; nfeat];
            // Same reasoning as bots/neural/train.rs's own needless_range_loop allows:
            // this is dense training-math indexing several parallel arrays (`scores`,
            // `p`, `grad`, `f = d.candidate(i, nfeat)`) together; a hand-zipped
            // rewrite risks silently misaligning gradient math with no compiler
            // signal, which is worse than the style lint it would silence.
            #[allow(clippy::needless_range_loop)]
            for i in 0..d.n {
                let f = d.candidate(i, nfeat);
                let coef = p[i] - if i == d.human_index { 1.0 } else { 0.0 };
                if coef == 0.0 {
                    continue;
                }
                for j in 0..nfeat {
                    grad[j] += coef * ((f[j] as f64 - mean[j]) / std[j]);
                }
            }
            for j in 0..nfeat {
                grad[j] += L2 * w[j];
            }

            t_step += 1;
            let bias1 = 1.0 - ADAM_BETA1.powi(t_step);
            let bias2 = 1.0 - ADAM_BETA2.powi(t_step);
            for j in 0..nfeat {
                adam_m[j] = ADAM_BETA1 * adam_m[j] + (1.0 - ADAM_BETA1) * grad[j];
                adam_v[j] = ADAM_BETA2 * adam_v[j] + (1.0 - ADAM_BETA2) * grad[j] * grad[j];
                let mhat = adam_m[j] / bias1;
                let vhat = adam_v[j] / bias2;
                w[j] -= LEARNING_RATE * mhat / (vhat.sqrt() + ADAM_EPS);
            }
        }
        eprintln!("epoch {epoch}: mean training loss = {:.4} over {n_steps} decisions", total_loss / n_steps as f64);
    }
    w
}

// ---------------------------------------------------------------- scoring

fn weights_to_vec(w: &Weights, nfeat: usize) -> Vec<f64> {
    let mut out = vec![0.0f64; nfeat];
    for &k in WeightKey::ALL {
        out[k as usize] = w.get(k);
    }
    out
}

fn vec_to_weights(v: &[f64]) -> Weights {
    let mut w = Weights::default();
    for &k in WeightKey::ALL {
        w.set(k, v[k as usize]);
    }
    w
}

/// The argmax candidate under `w`, first-candidate-wins on a tie -- mirrors
/// [`tta::bots::weighted::eval::WeightedBot::choose`]'s own tie-break
/// exactly, so an all-zero `w` (every candidate scores 0.0) resolves to
/// "the first legal move in `legal_moves`' own order", a deterministic
/// floor, not an undefined one.
fn top1(w: &[f64], nfeat: usize, d: &CachedDecision) -> usize {
    let mut best_i = 0usize;
    let mut best_v = f64::NEG_INFINITY;
    for i in 0..d.n {
        let f = d.candidate(i, nfeat);
        let mut v = 0.0f64;
        for j in 0..nfeat {
            v += w[j] * f[j] as f64;
        }
        if v > best_v {
            best_v = v;
            best_i = i;
        }
    }
    best_i
}

// --------------------------------------------------------------- reporting

#[derive(Default, Clone, Copy)]
struct Tally {
    n: u64,
    agree: u64,
}

impl Tally {
    fn pct(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            100.0 * self.agree as f64 / self.n as f64
        }
    }
}

struct Measurement {
    overall: Tally,
    by_category: [Tally; 11],
    by_age: [Tally; 5],
}

fn measure(w: &[f64], nfeat: usize, decisions: &[CachedDecision]) -> Measurement {
    let mut m = Measurement { overall: Tally::default(), by_category: [Tally::default(); 11], by_age: [Tally::default(); 5] };
    for d in decisions {
        let picked = top1(w, nfeat, d);
        let agreed = picked == d.human_index;
        m.overall.n += 1;
        m.overall.agree += agreed as u64;
        let c = &mut m.by_category[d.category as usize];
        c.n += 1;
        c.agree += agreed as u64;
        let a = &mut m.by_age[d.age as usize];
        a.n += 1;
        a.agree += agreed as u64;
    }
    m
}

fn print_measurement(label: &str, m: &Measurement) {
    println!("{label}: {:.1}% ({}/{})", m.overall.pct(), m.overall.agree, m.overall.n);
    for (i, name) in CATEGORY_NAMES.iter().enumerate() {
        let t = m.by_category[i];
        if t.n > 0 {
            println!("  category {name:<22} {:.1}% ({}/{})", t.pct(), t.agree, t.n);
        }
    }
    for (i, name) in AGE_NAMES.iter().enumerate() {
        let t = m.by_age[i];
        if t.n > 0 {
            println!("  age      {name:<22} {:.1}% ({}/{})", t.pct(), t.agree, t.n);
        }
    }
}

/// The blind-spot scan Paul's steer asked for: TRAIN-set decisions the
/// fitted vector -- which had every chance to price them, unlike a held-out
/// game -- still ranks the human's move far from the top on, grouped by
/// category. A training-set failure is not "the fit hasn't converged
/// there"; the whole training set is exactly what the fit optimized
/// against, so a bad rank there is the strongest evidence a concrete
/// decision shape needs a feature that does not exist yet, not just a
/// different number on an existing one.
fn print_blindspots(w: &[f64], nfeat: usize, train: &[CachedDecision]) {
    println!();
    println!("Blind-spot scan (TRAIN set, fitted weights, worst-first, one per category):");
    for (code, name) in CATEGORY_NAMES.iter().enumerate() {
        if !is_weak_category(code as u8) {
            continue;
        }
        let mut worst: Option<(f64, u32, u32, usize, usize)> = None; // (gap, game_id, lineno, human_rank, n)
        for d in train {
            if d.category as usize != code || d.n < 3 {
                continue;
            }
            let mut scores: Vec<(usize, f64)> = (0..d.n)
                .map(|i| {
                    let f = d.candidate(i, nfeat);
                    let s: f64 = (0..nfeat).map(|j| w[j] * f[j] as f64).sum();
                    (i, s)
                })
                .collect();
            scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let human_rank = scores.iter().position(|&(i, _)| i == d.human_index).unwrap_or(d.n - 1);
            let top_score = scores[0].1;
            let human_score = scores[human_rank].1;
            let gap = top_score - human_score;
            if worst.is_none_or(|(g, ..)| gap > g) {
                worst = Some((gap, d.game_id, d.lineno, human_rank + 1, d.n));
            }
        }
        if let Some((gap, game_id, lineno, rank, n)) = worst {
            println!(
                "  {name:<22} worst: game {game_id} line {lineno} -- human's move ranked {rank}/{n} \
                 by the fitted score, {gap:.2} points behind the fitted top pick"
            );
        }
    }
}

// -------------------------------------------------------------------- main

fn load_champion_set(p2: &str, p3: &str, p4: &str) -> Result<HashMap<u8, Weights>, String> {
    let mut m = HashMap::new();
    m.insert(2u8, load_weights(Path::new(p2))?);
    m.insert(3u8, load_weights(Path::new(p3))?);
    m.insert(4u8, load_weights(Path::new(p4))?);
    Ok(m)
}

fn run(args: &[String]) -> Result<(), String> {
    if args.len() < 7 {
        return Err(
            "usage: agreefit <index.tsv> <journals_dir> <champ_2p.json> <champ_3p.json> <champ_4p.json> \
             <cache_dir> <out_dir>"
                .to_string(),
        );
    }
    let index_path = &args[0];
    let journals_dir = &args[1];
    let champ = load_champion_set(&args[2], &args[3], &args[4])?;
    let cache_dir = PathBuf::from(&args[5]);
    let out_dir = PathBuf::from(&args[6]);
    fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;

    let nfeat = WeightKey::ALL.len();
    eprintln!("feature width: {nfeat}");

    let all_games = corpus::parse_index(index_path)?;
    let (train_games, holdout_games) = select_games(&all_games, journals_dir);
    eprintln!("selected {} train games, {} held-out games (Warlord/Emperor, hashed split)", train_games.len(), holdout_games.len());

    let card_index = build_card_index();
    let train_cache = cache_dir.join("train.agf");
    let holdout_cache = cache_dir.join("holdout.agf");

    let (ng, nd) = extract_split(&train_games, journals_dir, &card_index, &champ, &train_cache)?;
    eprintln!("train extraction: {ng} games replayed, {nd} decisions cached");
    let (ng, nd) = extract_split(&holdout_games, journals_dir, &card_index, &champ, &holdout_cache)?;
    eprintln!("held-out extraction: {ng} games replayed, {nd} decisions cached");

    let (nfeat_train, train) = read_cache(&train_cache)?;
    let (nfeat_holdout, holdout) = read_cache(&holdout_cache)?;
    if nfeat_train != nfeat || nfeat_holdout != nfeat {
        return Err(format!("cache feature width mismatch: train={nfeat_train} holdout={nfeat_holdout} expected={nfeat}"));
    }

    let (mean, std) = standardize(&train, nfeat);
    let w_scaled = train_softmax(&train, nfeat, &mean, &std);
    let w_fitted: Vec<f64> = (0..nfeat).map(|j| w_scaled[j] / std[j]).collect();

    let w_zero = vec![0.0f64; nfeat];
    // A single pooled champion vector for HELD-OUT comparison purposes only
    // (per-decision baseline measurement below still uses each decision's
    // OWN player-count champion -- see `measure_champion_baseline`).

    println!();
    println!("=== HELD-OUT ({} decisions) ===", holdout.len());
    let m_zero = measure(&w_zero, nfeat, &holdout);
    print_measurement("zero/uniform weights", &m_zero);
    println!();
    let m_fitted = measure(&w_fitted, nfeat, &holdout);
    print_measurement("fitted weights", &m_fitted);
    println!();
    // Champion baseline: per-decision, dot the RIGHT player-count champion
    // against that decision's own cached feature vectors (frozen at that
    // SAME champion during extraction, so this reproduces `evaluate` exactly
    // -- see `eval::linear_features`'s own doc comment).
    let m_champ = measure_by_player_champion(nfeat, &holdout, &champ, &train_games, &holdout_games);
    print_measurement("champion weights (reproduction check)", &m_champ);

    println!();
    println!("=== TRAIN ({} decisions) ===", train.len());
    let m_fitted_train = measure(&w_fitted, nfeat, &train);
    print_measurement("fitted weights", &m_fitted_train);

    print_blindspots(&w_fitted, nfeat, &train);

    // Weight movement: champion vs fitted, largest |delta| first (2p
    // champion as the reference scale -- all three player counts share the
    // same default table, and this is a coarse "what moved" scan, not a
    // per-player-count claim).
    println!();
    println!("Largest champion -> fitted weight moves (2p champion as reference):");
    let champ2 = weights_to_vec(champ.get(&2).expect("2p champion loaded"), nfeat);
    let mut moves: Vec<(f64, WeightKey, f64, f64)> = WeightKey::ALL
        .iter()
        .map(|&k| {
            let c = champ2[k as usize];
            let f = w_fitted[k as usize];
            ((f - c).abs(), k, c, f)
        })
        .collect();
    moves.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    for &(delta, k, c, f) in moves.iter().take(20) {
        println!("  {:<28} champion={c:.3} fitted={f:.3} (|delta|={delta:.3})", k.name());
    }

    // Save the fitted vector, gauntlet-JSON-compatible, one shared vector
    // duplicated across all three player counts (see `main`'s doc comment).
    let fitted_weights = vec_to_weights(&w_fitted);
    for players in [2u8, 3, 4] {
        let path = out_dir.join(format!("fitted_{players}p.json"));
        save_weights(&path, &fitted_weights, &[("gen", 0.0), ("players", f64::from(players))])?;
        eprintln!("wrote {}", path.display());
    }

    Ok(())
}

/// Held-out measurement against the CHAMPION, scored per-decision with that
/// decision's own player-count champion (train/holdout game lists passed
/// only so a `game_id -> players` lookup can be built without re-parsing the
/// journal -- the cached features themselves carry no player-count field).
fn measure_by_player_champion(
    nfeat: usize,
    decisions: &[CachedDecision],
    champ: &HashMap<u8, Weights>,
    train_games: &[&GameMeta],
    holdout_games: &[&GameMeta],
) -> Measurement {
    let mut players_by_id: HashMap<u32, u8> = HashMap::new();
    for g in train_games.iter().chain(holdout_games.iter()) {
        if let Ok(id) = g.id.parse::<u32>() {
            players_by_id.insert(id, g.players);
        }
    }
    let w_by_players: HashMap<u8, Vec<f64>> =
        champ.iter().map(|(&p, w)| (p, weights_to_vec(w, nfeat))).collect();

    let mut m = Measurement { overall: Tally::default(), by_category: [Tally::default(); 11], by_age: [Tally::default(); 5] };
    for d in decisions {
        let players = players_by_id.get(&d.game_id).copied().unwrap_or(2);
        let w = w_by_players.get(&players).expect("every player count has a loaded champion");
        let picked = top1(w, nfeat, d);
        let agreed = picked == d.human_index;
        m.overall.n += 1;
        m.overall.agree += agreed as u64;
        let c = &mut m.by_category[d.category as usize];
        c.n += 1;
        c.agree += agreed as u64;
        let a = &mut m.by_age[d.age as usize];
        a.n += 1;
        a.agree += agreed as u64;
    }
    m
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_id_is_deterministic_and_spreads_consecutive_ids() {
        let a = hash_id("7523818");
        let b = hash_id("7523819");
        assert_eq!(a, hash_id("7523818"), "must be a pure function of the id text");
        assert_ne!(a, b, "consecutive ids must not hash to the same bucket");
    }

    #[test]
    fn top1_breaks_ties_by_keeping_the_first_candidate() {
        let nfeat = 3;
        let d = CachedDecision {
            game_id: 1,
            lineno: 1,
            category: 0,
            age: 0,
            human_index: 1,
            n: 3,
            candidates: vec![0.0; 3 * nfeat],
        };
        let w = vec![0.0f64; nfeat];
        assert_eq!(top1(&w, nfeat, &d), 0, "an all-zero score must keep the first candidate, not the human's");
    }

    #[test]
    fn top1_picks_the_strictly_highest_scoring_candidate() {
        let nfeat = 2;
        let mut candidates = vec![0.0f32; 3 * nfeat];
        candidates[0..nfeat].copy_from_slice(&[1.0, 0.0]);
        candidates[nfeat..2 * nfeat].copy_from_slice(&[0.0, 5.0]);
        candidates[2 * nfeat..3 * nfeat].copy_from_slice(&[2.0, 0.0]);
        let d = CachedDecision { game_id: 1, lineno: 1, category: 0, age: 0, human_index: 1, n: 3, candidates };
        let w = vec![1.0f64, 1.0f64];
        assert_eq!(top1(&w, nfeat, &d), 1, "candidate 1 scores 5.0, strictly above the other two");
    }

    #[test]
    fn weights_to_vec_and_back_round_trips_every_key() {
        let mut w = Weights::default();
        w.set(WeightKey::Culture, 2.5);
        w.set(WeightKey::HandPotential, -1.25);
        let v = weights_to_vec(&w, WeightKey::ALL.len());
        let back = vec_to_weights(&v);
        for &k in WeightKey::ALL {
            assert_eq!(back.get(k), w.get(k), "{} did not round-trip", k.name());
        }
    }

    #[test]
    fn standardize_of_a_single_constant_feature_falls_back_to_scale_one() {
        let nfeat = 1;
        let d = CachedDecision {
            game_id: 1,
            lineno: 1,
            category: 0,
            age: 0,
            human_index: 0,
            n: 2,
            candidates: vec![3.0, 3.0],
        };
        let (mean, std) = standardize(&[d], nfeat);
        assert_eq!(mean[0], 3.0);
        assert_eq!(std[0], 1.0, "a zero-variance feature must fall back to scale 1.0, not divide by zero");
    }

    #[test]
    fn is_weak_category_matches_the_four_named_categories_only() {
        assert!(is_weak_category(category_code(Category::TakeCard)));
        assert!(is_weak_category(category_code(Category::IncreasePopulation)));
        assert!(is_weak_category(category_code(Category::LeaderOrWonderStep)));
        assert!(is_weak_category(category_code(Category::AggressionOrWar)));
        assert!(!is_weak_category(category_code(Category::Build)));
        assert!(!is_weak_category(category_code(Category::EndTurn)));
    }
}
