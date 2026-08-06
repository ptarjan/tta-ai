//! One bot-spec grammar for every caller that has to seat "whatever the
//! operator named on the command line" -- classical evaluator or value-net
//! checkpoint -- at a table.
//!
//! Ports the SUBSTANCE of `experiments/arena.py::load_spec`/`make_bot`, which
//! was the only place in the repo that could say "`plan:champion_2p.json,
//! width=8`" or "`nplan:best_search.pt`". `bots::greedy::make_seats` already
//! parses a spec, but a deliberately smaller one: bare [`BotKind`] names,
//! cycled over seats, with the weight vector supplied separately by the
//! caller. That is exactly right for `selfplay`/`rankdata`, where every seat
//! reads the same vector; it cannot express the two things the neural search
//! loop is built out of:
//!
//!   * a **checkpoint** as a player (`neural:` = the 1-ply argmax over the
//!     net, `nplan:` = `PlanBot`'s whole-turn beam with the net as its leaf),
//!     which `BotKind` has no variant for at all; and
//!   * **per-side search knobs** (`width=`, `nodes=`), which matter because
//!     `make_seats` pins every `plan` seat to `width: 2` -- the league's
//!     training convention -- while the loop's anchor match is defined as
//!     `plan:champion_2p,width=8`, a four-times-wider beam and a
//!     meaningfully stronger bot.
//!
//! So this module is `make_seats`'s grammar plus those two, and it does NOT
//! replace it: a caller that only needs "N seats of one kind, one vector"
//! should keep using `make_seats`.
//!
//! # Why parsing and loading are two steps
//!
//! [`Spec::parse`] is pure string work and cannot fail for I/O reasons;
//! [`Spec::load`] is the step that touches the disk. Splitting them means a
//! command line with a typo in it is rejected before a 300MB checkpoint is
//! read, and -- more importantly -- means a caller can parse BOTH sides of a
//! match up front and only then start loading, so an unusable spec never
//! produces a half-set-up run. `bin/rankdata.rs` already establishes this
//! shape ("fail on a bad spec here, not inside a worker thread").
//!
//! # Why a knob that does not apply is an ERROR
//!
//! `--opponent weighted,width=8` is asking for a beam and getting a 1-ply
//! evaluator. Python's `load_spec` builds a kwargs dict and lets the
//! constructor ignore what it does not recognise, so that command line runs
//! and silently measures the wrong bot -- and an anchor match that silently
//! measured `width=2` instead of `width=8` is precisely the class of error
//! `docs/NEURAL_SEARCH_LOOP.md`'s curve-comment convention exists to clean up
//! after. [`Spec::parse`] instead rejects every (kind, knob) pair that would
//! be ignored, by exhaustive `match`, so the set of knobs a kind honours is a
//! compile-time fact rather than a documentation claim.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::bots::greedy::{BotKind, GreedyBot, RandomBot};
use crate::bots::pending;
use crate::bots::plan;
use crate::bots::quiescent;
use crate::bots::weighted;
use crate::bots::weighted::eval::load_weights;
use crate::bots::weighted::weights::Weights;
use crate::legal;
use crate::moves::Move;
use crate::rng::PyRandom;
use crate::state::GameState;

use super::bot as neural_bot;
use super::net::{load_checkpoint, ValueNet};
use super::plan as neural_plan;

// ======================================================================= kind

/// What sort of player a spec names. Three variants rather than seven, so
/// every "does this knob apply?" question below is answered once per FAMILY
/// (linear evaluator / net + 1-ply / net + beam) instead of once per bot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One of `bots::greedy`'s classical bots, reading a linear weight vector.
    Classical(BotKind),
    /// `neural:CKPT` -- [`neural_bot::pick`], the net's own 1-ply argmax.
    Neural,
    /// `nplan:CKPT` -- [`neural_plan::pick`], the whole-turn beam with the net
    /// as its leaf evaluator. This is the policy the search loop ships.
    NeuralPlan,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Classical(k) => k.name(),
            Kind::Neural => "neural",
            Kind::NeuralPlan => "nplan",
        }
    }

    fn parse(s: &str) -> Result<Kind, String> {
        match s {
            "neural" => Ok(Kind::Neural),
            "nplan" => Ok(Kind::NeuralPlan),
            other => BotKind::from_str(other).map(Kind::Classical).map_err(|e| {
                format!("{e} (or the checkpoint-backed kinds neural, nplan)")
            }),
        }
    }

    /// Does this kind read a `PATH` after the colon, and what is that path?
    /// A classical kind reads a weight vector; the two neural kinds read a
    /// checkpoint. Nothing reads both, which is why one field carries it.
    fn path_is_checkpoint(self) -> bool {
        match self {
            Kind::Classical(_) => false,
            Kind::Neural | Kind::NeuralPlan => true,
        }
    }
}

// ======================================================================= knob

/// A `key=value` search knob. An enum, not a string key, so [`Spec::parse`]'s
/// applicability check below is an exhaustive `match` the compiler will
/// re-check the day a new knob is added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Knob {
    /// Beam width kept between plies (`plan`, `nplan`).
    Width,
    /// Hard cap on `apply` calls per root decision (`nplan`).
    Nodes,
    /// Re-shuffle the piles the mover cannot see before searching. Off is a
    /// deliberate A/B that MEASURES the hidden-information leak; it is never
    /// how a bot should play for real.
    Determinize,
    /// Score bonus added to `end_turn` (`neural` only).
    EndTurnBias,
    /// Price a declared war through its resolution (`plan`, `nplan`).
    War,
}

impl Knob {
    const ALL: &'static [Knob] =
        &[Knob::Width, Knob::Nodes, Knob::Determinize, Knob::EndTurnBias, Knob::War];

    const fn name(self) -> &'static str {
        match self {
            Knob::Width => "width",
            Knob::Nodes => "nodes",
            Knob::Determinize => "det",
            Knob::EndTurnBias => "etb",
            Knob::War => "war",
        }
    }

    fn parse(s: &str) -> Result<Knob, String> {
        Knob::ALL.iter().copied().find(|k| k.name() == s).ok_or_else(|| {
            format!(
                "unknown knob {s:?}, want one of {}",
                Knob::ALL.iter().map(|k| k.name()).collect::<Vec<_>>().join(", ")
            )
        })
    }

    /// Whether `kind` actually reads this knob. Exhaustive on BOTH axes: a
    /// new [`Kind`] or a new [`Knob`] fails to compile until someone decides,
    /// in this one table, whether the combination means anything.
    fn applies_to(self, kind: Kind) -> bool {
        match (kind, self) {
            // The classical beam honours width, determinization and war
            // lookahead; it has no node cap knob (`PlanConfig::max_nodes`
            // exists but the loop has never set it) and no end-turn bias.
            (Kind::Classical(BotKind::Plan), Knob::Width)
            | (Kind::Classical(BotKind::Plan), Knob::Determinize)
            | (Kind::Classical(BotKind::Plan), Knob::War) => true,
            (Kind::Classical(BotKind::Plan), Knob::Nodes)
            | (Kind::Classical(BotKind::Plan), Knob::EndTurnBias) => false,
            // Quiescence search determinizes nothing and has no beam.
            (Kind::Classical(BotKind::Quiescent), _) => false,
            (Kind::Classical(BotKind::Random), _)
            | (Kind::Classical(BotKind::Greedy), _)
            | (Kind::Classical(BotKind::Weighted), _) => false,
            // The 1-ply neural bot: determinization and the end-turn bias are
            // its only two knobs (`NeuralBotConfig`'s only two fields besides
            // `allow_resign`, which no caller has ever varied).
            (Kind::Neural, Knob::Determinize) | (Kind::Neural, Knob::EndTurnBias) => true,
            (Kind::Neural, Knob::Width) | (Kind::Neural, Knob::Nodes) | (Kind::Neural, Knob::War) => {
                false
            }
            // The neural beam reads everything except the end-turn bias.
            (Kind::NeuralPlan, Knob::Width)
            | (Kind::NeuralPlan, Knob::Nodes)
            | (Kind::NeuralPlan, Knob::Determinize)
            | (Kind::NeuralPlan, Knob::War) => true,
            (Kind::NeuralPlan, Knob::EndTurnBias) => false,
        }
    }
}

// ======================================================================= spec

/// A parsed, not-yet-loaded player description.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub kind: Kind,
    /// Weight vector (classical kinds) or checkpoint (`neural`/`nplan`).
    /// `None` means "this build's defaults", which for a checkpoint kind is
    /// not a thing that exists -- [`Spec::parse`] rejects it there.
    pub path: Option<PathBuf>,
    width: Option<usize>,
    nodes: Option<i64>,
    determinize: Option<bool>,
    end_turn_bias: Option<f64>,
    war: Option<bool>,
    /// The spec exactly as written, for reports and shard teacher tags. Kept
    /// so a log line names what the operator typed rather than a
    /// reconstruction of it that could differ.
    text: String,
}

fn parse_bool(v: &str, knob: Knob) -> Result<bool, String> {
    match v {
        "1" | "true" | "yes" => Ok(true),
        "0" | "false" | "no" => Ok(false),
        other => Err(format!("{}={other:?} is not a boolean (0/1)", knob.name())),
    }
}

fn parse_num<T: FromStr>(v: &str, knob: Knob) -> Result<T, String> {
    v.parse().map_err(|_| format!("{}={v:?} is not a number", knob.name()))
}

impl Spec {
    /// `KIND[:PATH][,KEY=VALUE]...`
    ///
    /// ```text
    /// weighted
    /// plan:analysis/frozen/champion_2p.json,width=8
    /// nplan:checkpoints/best_search.ckpt,width=8,nodes=1200
    /// neural:checkpoints/cand.ckpt,det=0
    /// ```
    pub fn parse(text: &str) -> Result<Spec, String> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err("empty bot spec".to_string());
        }
        let mut parts = trimmed.split(',');
        let head = parts.next().unwrap_or("");
        let (kind_str, path_str) = match head.split_once(':') {
            Some((k, p)) => (k.trim(), Some(p.trim())),
            None => (head.trim(), None),
        };
        let kind = Kind::parse(kind_str)?;

        let path = match path_str {
            Some(p) if p.is_empty() => {
                return Err(format!("{kind_str}: the ':' is there but the path after it is empty"))
            }
            Some(p) => Some(PathBuf::from(p)),
            None if kind.path_is_checkpoint() => {
                return Err(format!(
                    "{kind_str} needs a checkpoint: write {kind_str}:PATH -- there is no built-in net"
                ))
            }
            None => None,
        };

        let mut spec = Spec {
            kind,
            path,
            width: None,
            nodes: None,
            determinize: None,
            end_turn_bias: None,
            war: None,
            text: trimmed.to_string(),
        };
        for part in parts {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| format!("{part:?} is not KEY=VALUE"))?;
            let knob = Knob::parse(key.trim())?;
            let value = value.trim();
            if !knob.applies_to(kind) {
                // See this module's top doc comment: silently ignoring this
                // is how a run measures a bot nobody asked for.
                return Err(format!(
                    "{} does not read {}=; a spec that would be ignored is refused rather than run",
                    kind.name(),
                    knob.name()
                ));
            }
            match knob {
                Knob::Width => spec.width = Some(parse_num(value, knob)?),
                Knob::Nodes => spec.nodes = Some(parse_num(value, knob)?),
                Knob::Determinize => spec.determinize = Some(parse_bool(value, knob)?),
                Knob::EndTurnBias => spec.end_turn_bias = Some(parse_num(value, knob)?),
                Knob::War => spec.war = Some(parse_bool(value, knob)?),
            }
        }
        if let Some(0) = spec.width {
            return Err("width=0 would search nothing".to_string());
        }
        Ok(spec)
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Read whatever this spec points at. The one step that touches the disk.
    pub fn load(&self) -> Result<Contender, String> {
        let path = self.path.as_deref();
        match self.kind {
            Kind::Classical(kind) => {
                let weights = match path {
                    Some(p) => load_weights(p)?,
                    None => Weights::defaults(),
                };
                Ok(Contender::Classical {
                    kind,
                    weights,
                    // A `plan` seat built through `greedy::build_bots` is
                    // pinned to width 2 (the league's training convention);
                    // a spec that says nothing gets `PlanConfig`'s own
                    // default instead, because this grammar exists for
                    // callers who name a beam explicitly.
                    width: self.width.unwrap_or(plan::PlanConfig::default().width),
                    determinize: self.determinize.unwrap_or(true),
                    war: self.war.unwrap_or(plan::PlanConfig::default().war_lookahead),
                    text: self.text.clone(),
                })
            }
            Kind::Neural => {
                let net = self.load_net(path)?;
                let base = neural_bot::NeuralBotConfig::default();
                Ok(Contender::Neural {
                    net,
                    cfg: neural_bot::NeuralBotConfig {
                        determinize: self.determinize.unwrap_or(base.determinize),
                        end_turn_bias: self.end_turn_bias.unwrap_or(base.end_turn_bias),
                        allow_resign: base.allow_resign,
                    },
                    text: self.text.clone(),
                })
            }
            Kind::NeuralPlan => {
                let net = self.load_net(path)?;
                let base = neural_plan::NeuralPlanConfig::default();
                let mut cfg = neural_plan::NeuralPlanConfig {
                    width: self.width.unwrap_or(base.width),
                    max_nodes: self.nodes.unwrap_or(base.max_nodes),
                    war_lookahead: self.war.unwrap_or(base.war_lookahead),
                    ..base
                };
                cfg.bot.determinize = self.determinize.unwrap_or(cfg.bot.determinize);
                Ok(Contender::NeuralPlan { net, cfg, text: self.text.clone() })
            }
        }
    }

    fn load_net(&self, path: Option<&Path>) -> Result<ValueNet, String> {
        let p = path.ok_or_else(|| format!("{}: no checkpoint path", self.kind.name()))?;
        let (net, _meta) = load_checkpoint(p)?;
        Ok(net)
    }
}

// ================================================================== contender

/// A loaded player: everything a seat needs except its per-game RNG stream
/// and counters. Shared by reference across every thread of a match, so the
/// (large) [`ValueNet`] is read once and never cloned per game.
#[derive(Debug)]
pub enum Contender {
    Classical {
        kind: BotKind,
        weights: Weights,
        width: usize,
        determinize: bool,
        war: bool,
        text: String,
    },
    Neural {
        net: ValueNet,
        cfg: neural_bot::NeuralBotConfig,
        text: String,
    },
    NeuralPlan {
        net: ValueNet,
        cfg: neural_plan::NeuralPlanConfig,
        text: String,
    },
}

impl Contender {
    /// Convenience for the common `Spec::parse(s)?.load()?`.
    pub fn parse_and_load(text: &str) -> Result<Contender, String> {
        Spec::parse(text)?.load()
    }

    /// The spec that produced this, for reports.
    pub fn text(&self) -> &str {
        match self {
            Contender::Classical { text, .. }
            | Contender::Neural { text, .. }
            | Contender::NeuralPlan { text, .. } => text,
        }
    }

    /// Does this contender carry a net a caller could take a 1-ply argmax
    /// with? The generator's `DISAGREE` health meter needs one, and asking
    /// this is how it says "I cannot measure that for this teacher" instead
    /// of reporting a zero that reads as "the search never overruled it".
    pub fn net(&self) -> Option<&ValueNet> {
        match self {
            Contender::Classical { .. } => None,
            Contender::Neural { net, .. } | Contender::NeuralPlan { net, .. } => Some(net),
        }
    }

    /// A fresh player for one seat of one game. Borrows the net rather than
    /// copying it.
    pub fn seat(&self, seed: i64) -> Player<'_> {
        match self {
            Contender::Classical { kind, weights, width, determinize, war, .. } => {
                Player::Classical(build_classical(*kind, *weights, *width, *determinize, *war, seed))
            }
            Contender::Neural { net, cfg, .. } => Player::Neural {
                net,
                cfg: *cfg,
                stats: neural_bot::Stats::default(),
                rng: PyRandom::new(seed),
            },
            Contender::NeuralPlan { net, cfg, .. } => Player::NeuralPlan {
                net,
                cfg: *cfg,
                stats: neural_plan::Stats::default(),
                counters: pending::Counters::default(),
                rng: PyRandom::new(seed),
            },
        }
    }
}

/// `greedy::build_bots` for ONE seat, honouring this grammar's search knobs.
/// Not a call into `build_bots` itself: that function pins `plan` to width 2
/// and offers no way to say otherwise, which is the whole reason this module
/// exists.
fn build_classical(
    kind: BotKind,
    weights: Weights,
    width: usize,
    determinize: bool,
    war: bool,
    seed: i64,
) -> ClassicalPlayer {
    match kind {
        BotKind::Random => ClassicalPlayer::Random(RandomBot::new(seed as u64)),
        BotKind::Greedy => ClassicalPlayer::Greedy(GreedyBot::default()),
        BotKind::Weighted => ClassicalPlayer::Weighted(weighted::eval::WeightedBot::new(weights)),
        BotKind::Quiescent => ClassicalPlayer::Quiescent {
            cfg: quiescent::QuiescenceConfig::default(),
            weights,
            stats: quiescent::Stats::default(),
        },
        BotKind::Plan => {
            let base = plan::PlanConfig::default();
            let mut cfg = plan::PlanConfig { width, weights, war_lookahead: war, ..base };
            cfg.bot.determinize = determinize;
            ClassicalPlayer::Plan {
                cfg,
                stats: plan::Stats::default(),
                counters: pending::Counters::default(),
                rng: PyRandom::new(seed),
            }
        }
    }
}

/// The classical half of [`Player`]. A near-twin of `greedy::Bot`, which
/// cannot be reused directly because its `Plan` variant is only ever built by
/// `build_bots` at width 2 and its fields are private to that construction.
///
/// No `Debug`: [`PyRandom`] has none, matching `greedy::Bot`, which is not
/// `Debug` either for the same reason.
pub enum ClassicalPlayer {
    Random(RandomBot),
    Greedy(GreedyBot),
    Weighted(weighted::eval::WeightedBot),
    Quiescent { cfg: quiescent::QuiescenceConfig, weights: Weights, stats: quiescent::Stats },
    Plan { cfg: plan::PlanConfig, stats: plan::Stats, counters: pending::Counters, rng: PyRandom },
}

/// The positions a search actually PRICED at one decision, in whatever form
/// that search's own scorer already had them.
///
/// Two shapes rather than one because the two beams differ in what they hold:
/// `bots::neural::plan` has already built the leaf ENCODING (collecting it is
/// a clone), while `bots::plan` scores a linear evaluator straight off the
/// STATE and has no encoding to hand over. Normalising both to encodings is
/// [`Player::leaf_encodings`]'s job, and it is done there rather than in
/// either search because only the caller knows whose viewpoint to encode
/// from.
pub enum Leaves {
    /// This player has no search to collect from -- a 1-ply evaluator prices
    /// exactly the candidate children the caller already has. NOT "nothing
    /// happened": it is why [`super::rankdata`] falls back to a pre-move
    /// value anchor and says so.
    NoSearch,
    States(Vec<GameState>),
    Encodings(Vec<Vec<f64>>),
}

/// One seated player, ready to decide.
pub enum Player<'a> {
    Classical(ClassicalPlayer),
    Neural {
        net: &'a ValueNet,
        cfg: neural_bot::NeuralBotConfig,
        stats: neural_bot::Stats,
        rng: PyRandom,
    },
    NeuralPlan {
        net: &'a ValueNet,
        cfg: neural_plan::NeuralPlanConfig,
        stats: neural_plan::Stats,
        counters: pending::Counters,
        rng: PyRandom,
    },
}

impl Player<'_> {
    /// Decide from `state`, generating the legal list itself -- the shape
    /// `greedy::Bot::pick` already uses.
    pub fn pick(&mut self, state: &GameState) -> Move {
        let moves = legal::legal_moves(state);
        self.pick_from(state, moves.as_slice())
    }

    /// Decide among an ALREADY-GENERATED move list. The data generator needs
    /// this: it filters `resign` out of the candidate set and then has to
    /// rank exactly the moves it filtered, so a second, independently
    /// generated list inside `pick` could disagree with the one being
    /// labelled.
    ///
    /// # Panics
    /// If `moves` is empty -- every bot in this crate treats that as a caller
    /// bug rather than returning an `Option` nothing could act on.
    pub fn pick_from(&mut self, state: &GameState, moves: &[Move]) -> Move {
        match self {
            Player::Classical(bot) => match bot {
                ClassicalPlayer::Random(b) => b.choose(moves),
                ClassicalPlayer::Greedy(b) => b.choose(state, moves),
                ClassicalPlayer::Weighted(b) => b.choose(state, moves),
                ClassicalPlayer::Quiescent { cfg, weights, stats } => {
                    let w = *weights;
                    let eval =
                        move |s: &GameState, i: u8| weighted::eval::evaluate(s, i, &w, None, None);
                    quiescent::pick(cfg, stats, state, moves, &eval)
                }
                ClassicalPlayer::Plan { cfg, stats, counters, rng } => {
                    plan::pick(cfg, stats, counters, rng, state, moves)
                }
            },
            Player::Neural { net, cfg, stats, rng } => {
                neural_bot::pick(cfg, net, stats, rng, state, moves)
            }
            Player::NeuralPlan { net, cfg, stats, counters, rng } => {
                neural_plan::pick(cfg, net, stats, counters, rng, state, moves)
            }
        }
    }

    /// [`Player::pick_from`], also returning every position this player's
    /// search priced. A training-data generator needs those and nothing else
    /// will do: see `bots::plan::Bank`'s doc comment for the measured
    /// distribution bug that makes the pre-move state the wrong thing to
    /// train a beam's leaf evaluator on.
    pub fn pick_collecting(&mut self, state: &GameState, moves: &[Move]) -> (Move, Leaves) {
        match self {
            Player::Classical(ClassicalPlayer::Plan { cfg, stats, counters, rng }) => {
                let mut bank = plan::Bank::collecting();
                let mv = plan::pick_collecting(cfg, stats, counters, rng, state, moves, &mut bank);
                (mv, Leaves::States(bank.take()))
            }
            Player::NeuralPlan { net, cfg, stats, counters, rng } => {
                let mut bank = plan::Bank::collecting();
                let mv = neural_plan::pick_collecting(
                    cfg, net, stats, counters, rng, state, moves, &mut bank,
                );
                (mv, Leaves::Encodings(bank.take()))
            }
            // Everything else decides without a beam, so there is no leaf
            // distribution to sample. Listed by variant rather than caught by
            // a wildcard: the day a new searching player is added, this match
            // stops compiling until someone decides whether it has leaves.
            Player::Classical(ClassicalPlayer::Random(_))
            | Player::Classical(ClassicalPlayer::Greedy(_))
            | Player::Classical(ClassicalPlayer::Weighted(_))
            | Player::Classical(ClassicalPlayer::Quiescent { .. })
            | Player::Neural { .. } => (self.pick_from(state, moves), Leaves::NoSearch),
        }
    }

    /// Normalise [`Leaves`] to the encodings the value net is SERVED at play
    /// time, from `me`'s viewpoint.
    ///
    /// Collected states go through `neural::plan::leaf_enc`, the same
    /// war-substitution the beam applies before asking the net anything --
    /// so a net trained on these rows is trained on the distribution it will
    /// actually be queried on, which is the entire point of collecting them.
    pub fn leaf_encodings(&self, leaves: Leaves, me: u8) -> Vec<Vec<f64>> {
        match leaves {
            Leaves::NoSearch => Vec::new(),
            Leaves::Encodings(encs) => encs,
            Leaves::States(states) => {
                let war = match self {
                    Player::Classical(ClassicalPlayer::Plan { cfg, .. }) => cfg.war_lookahead,
                    Player::NeuralPlan { cfg, .. } => cfg.war_lookahead,
                    Player::Classical(ClassicalPlayer::Random(_))
                    | Player::Classical(ClassicalPlayer::Greedy(_))
                    | Player::Classical(ClassicalPlayer::Weighted(_))
                    | Player::Classical(ClassicalPlayer::Quiescent { .. })
                    | Player::Neural { .. } => false,
                };
                let mut wars = 0u64;
                states.iter().map(|t| neural_plan::leaf_enc(t, me, war, &mut wars)).collect()
            }
        }
    }
}

/// Build one player per seat: `challenger` in seat `seat`, `defender` in
/// every other. The seat-rotation caller (`neural::eval`) is the only user,
/// but it lives here next to [`Contender::seat`] so the per-seat seeding
/// convention (`seed * 131 + i`, `greedy::build_bots`'s) has exactly one
/// definition.
pub fn seat_table<'a>(
    challenger: &'a Contender,
    defender: &'a Contender,
    players: u8,
    seat: usize,
    seed: i64,
) -> Vec<Player<'a>> {
    (0..players as usize)
        .map(|i| {
            let who = if i == seat { challenger } else { defender };
            who.seat(seed.wrapping_mul(131).wrapping_add(i as i64))
        })
        .collect()
}

// ===================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_kind_parses_with_no_path_and_no_knobs() {
        let s = Spec::parse("weighted").unwrap();
        assert_eq!(s.kind, Kind::Classical(BotKind::Weighted));
        assert_eq!(s.path, None);
    }

    #[test]
    fn the_anchor_matchs_own_spec_parses_with_its_width() {
        let s = Spec::parse("plan:analysis/frozen/champion_2p.json,width=8").unwrap();
        assert_eq!(s.kind, Kind::Classical(BotKind::Plan));
        assert_eq!(s.path, Some(PathBuf::from("analysis/frozen/champion_2p.json")));
        assert_eq!(s.width, Some(8));
    }

    #[test]
    fn a_checkpoint_kind_without_a_path_is_rejected_rather_than_defaulted() {
        for spec in ["neural", "nplan"] {
            let err = Spec::parse(spec).unwrap_err();
            assert!(err.contains("checkpoint"), "{spec}: {err}");
        }
    }

    #[test]
    fn a_colon_with_nothing_after_it_is_an_error_not_an_empty_path() {
        assert!(Spec::parse("plan:").is_err());
    }

    /// The point of the applicability table: a knob the bot would ignore is
    /// refused, because a silently-ignored `width=` measures a different bot
    /// than the operator asked for.
    #[test]
    fn a_knob_the_kind_cannot_read_is_refused_instead_of_ignored() {
        let err = Spec::parse("weighted,width=8").unwrap_err();
        assert!(err.contains("width"), "{err}");
        assert!(Spec::parse("neural:x.ckpt,width=8").is_err());
        assert!(Spec::parse("nplan:x.ckpt,etb=1.0").is_err());
    }

    #[test]
    fn every_knob_is_read_by_at_least_one_kind() {
        let kinds = [
            Kind::Classical(BotKind::Random),
            Kind::Classical(BotKind::Greedy),
            Kind::Classical(BotKind::Weighted),
            Kind::Classical(BotKind::Quiescent),
            Kind::Classical(BotKind::Plan),
            Kind::Neural,
            Kind::NeuralPlan,
        ];
        for knob in Knob::ALL {
            assert!(
                kinds.iter().any(|k| knob.applies_to(*k)),
                "{} applies to nothing, so it can never be set",
                knob.name()
            );
        }
    }

    #[test]
    fn an_unknown_kind_names_the_checkpoint_kinds_too() {
        let err = Spec::parse("mcts").unwrap_err();
        assert!(err.contains("nplan"), "{err}");
    }

    #[test]
    fn a_knob_without_an_equals_sign_is_an_error() {
        assert!(Spec::parse("plan,width").is_err());
    }

    #[test]
    fn a_non_numeric_width_is_an_error_rather_than_a_silent_zero() {
        assert!(Spec::parse("plan,width=wide").is_err());
        assert!(Spec::parse("plan,width=0").is_err());
    }

    #[test]
    fn determinization_can_be_switched_off_for_the_leak_measuring_ab() {
        let s = Spec::parse("plan,det=0").unwrap();
        assert_eq!(s.determinize, Some(false));
    }

    #[test]
    fn the_original_text_is_kept_verbatim_for_reports() {
        let c = Spec::parse(" plan,width=4 ").unwrap().load().unwrap();
        assert_eq!(c.text(), "plan,width=4");
    }

    /// A `plan` spec with no `width=` must NOT inherit `make_seats`'s width
    /// of 2: this grammar is for callers naming a beam on purpose.
    #[test]
    fn a_plan_spec_defaults_to_the_beams_own_width_not_the_leagues_two() {
        match Spec::parse("plan").unwrap().load().unwrap() {
            Contender::Classical { width, .. } => {
                assert_eq!(width, plan::PlanConfig::default().width);
                assert_ne!(width, 2, "the league's training width must not leak in here");
            }
            _ => panic!("a classical spec must load as a classical contender"),
        }
    }

    #[test]
    fn a_classical_contender_has_no_net_to_take_a_one_ply_argmax_with() {
        let c = Spec::parse("weighted").unwrap().load().unwrap();
        assert!(c.net().is_none());
    }

    #[test]
    fn a_seated_table_puts_the_challenger_in_exactly_one_seat() {
        let a = Spec::parse("greedy").unwrap().load().unwrap();
        let b = Spec::parse("weighted").unwrap().load().unwrap();
        let table = seat_table(&a, &b, 3, 1, 7);
        let is_greedy = |p: &Player<'_>| {
            matches!(p, Player::Classical(ClassicalPlayer::Greedy(_)))
        };
        assert_eq!(table.iter().filter(|p| is_greedy(p)).count(), 1);
        assert!(is_greedy(&table[1]));
    }
}
