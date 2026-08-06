//! What must the human actually type? Ask the evaluator, do not guess.
//!
//! The expensive part of a human-in-the-loop game is transcription. A
//! hardcoded "minimal field list" would be wrong the day the evaluator grows
//! a new eye, and wrong in the dangerous direction: the harness would stop
//! collecting something the bot has started reading, silently turning the
//! measurement into one of a bot playing blindfolded.
//!
//! So the list is not written down anywhere. It is *derived*, here, by
//! perturbation: take the live position, change one thing a human could
//! observe, re-score every legal move exactly the way the advisor's
//! `rank_moves` does, and see whether the bot's decision moved. If it did,
//! the human has to type it. If nothing moved -- not the chosen move, not
//! the ranking, not even the raw scores -- the human must not be asked.
//!
//! Adding a new observable to the harness means adding a [`Probe`] to
//! [`PROBES`]. Wiring a new *feature* into the evaluator means doing nothing
//! at all here: the probe that already covers that observable starts
//! reporting [`Verdict::Move`] on its own, and `harness::play` starts
//! prompting for it.
//!
//! Ported from `harness/fields.py`. One thing deliberately NOT carried over:
//! Python memoizes [`civil_pool_for_age`]'s table scan in a module-global
//! `_POOL_CACHE` dict, keyed by player count and never evicted -- exactly the
//! "lazily-populated global cache" this project's own style rules forbid.
//! The scan is a filter over 236 `const` table rows, cheap enough to redo on
//! every call; dropping the cache trades an unmeasurable slowdown for one
//! less piece of process-global mutable state.

use crate::advisor::advisor as adv;
use crate::apply;
use crate::bots::weighted::eval;
use crate::bots::weighted::rivals;
use crate::bots::weighted::weights::{WeightKey, Weights};
use crate::cards::{Age, CardId, CARDS};
use crate::game;
use crate::legal;
use crate::moves::Move;
use crate::state::{CardList, GameState, PlayerState, Phase, NOT_SEEDED};

// ---------------------------------------------------------------- verdicts

/// Ordered from "must not be asked for" to "must be typed". Declaration
/// order IS the strength order: `#[derive(Ord)]` compares variants by
/// position, so [`Verdict::worse`] is just `Ord::max`. Mirrors `RANKS`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Verdict {
    /// Byte-identical scores. The evaluator cannot see this. Asking a human
    /// for it is unpaid data entry.
    Inert,
    /// Scores moved but the ranking is identical. Advisory -- decision
    /// irrelevant for a 1-ply argmax, but a real dependency worth recording.
    Score,
    /// Same top move, different ordering below it. Mandatory: a different
    /// weight vector or a deeper search diverges here, and the whole point
    /// of logging is to re-score later.
    Rank,
    /// Same moves available, different one chosen. Mandatory.
    Move,
    /// The set of legal moves itself changed. Mandatory for a reason that
    /// has nothing to do with the evaluator: the bot cannot take a card the
    /// mirror does not have. Kept apart from `Move` so a report never
    /// implies the evaluator reads something it is blind to.
    Legal,
}

impl Verdict {
    /// The stronger (more demanding) of two verdicts.
    pub fn worse(self, other: Verdict) -> Verdict {
        self.max(other)
    }

    /// A verdict that means "the human must type this", whatever the reason.
    pub fn is_mandatory(self) -> bool {
        matches!(self, Verdict::Legal | Verdict::Move | Verdict::Rank)
    }

    /// A verdict that means "a feature reads it", as opposed to "the rules
    /// need it". Mirrors `EVALUATED`.
    pub fn is_evaluated(self) -> bool {
        matches!(self, Verdict::Move | Verdict::Rank | Verdict::Score)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Legal => "legal",
            Verdict::Move => "move",
            Verdict::Rank => "rank",
            Verdict::Score => "score",
            Verdict::Inert => "inert",
        }
    }
}

// ------------------------------------------------------------------ probes

/// What part of the position a probe's mutation touches. Mirrors `scope`
/// ("self" is listed in the Python dataclass's type but no probe ever uses
/// it, so it is not carried over here -- an unused enum arm is exactly the
/// kind of thing `match` exhaustiveness is supposed to catch, not hide).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scope {
    Rival,
    Shared,
}

/// One observable a human could read off the app screen, identified rather
/// than named: every place that used to hold a `probe.id` string now holds
/// one of these, so an unrecognised id is a compile error instead of a typo
/// nobody notices. `as_str` is the one place the original string survives,
/// for JSON records and CLI output.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ProbeId {
    RivalCulture,
    RivalCultureRate,
    RivalScienceRate,
    RivalStrength,
    RivalScience,
    RivalFood,
    RivalResources,
    RivalCivilActions,
    RivalMilitaryActions,
    RivalWorkersFree,
    RivalYellowBank,
    RivalHappy,
    RivalTechs,
    RivalWonders,
    RivalColonies,
    RivalWonderProgress,
    RivalGovernment,
    RivalHandCivilSize,
    RivalHandCivilIds,
    RivalHandMilitarySize,
    RowContents,
    RowOrder,
    RowOccupancy,
    EventsFuture,
    EventsCurrent,
    DeckCivilIdentity,
    DeckMilitaryDiscard,
}

impl ProbeId {
    pub fn as_str(self) -> &'static str {
        use ProbeId::*;
        match self {
            RivalCulture => "rival.culture",
            RivalCultureRate => "rival.culture_rate",
            RivalScienceRate => "rival.science_rate",
            RivalStrength => "rival.strength",
            RivalScience => "rival.science",
            RivalFood => "rival.food",
            RivalResources => "rival.resources",
            RivalCivilActions => "rival.civil_actions",
            RivalMilitaryActions => "rival.military_actions",
            RivalWorkersFree => "rival.workers_free",
            RivalYellowBank => "rival.yellow_bank",
            RivalHappy => "rival.happy",
            RivalTechs => "rival.techs",
            RivalWonders => "rival.wonders",
            RivalColonies => "rival.colonies",
            RivalWonderProgress => "rival.wonder_progress",
            RivalGovernment => "rival.government",
            RivalHandCivilSize => "rival.hand_civil_size",
            RivalHandCivilIds => "rival.hand_civil_ids",
            RivalHandMilitarySize => "rival.hand_military_size",
            RowContents => "row.contents",
            RowOrder => "row.order",
            RowOccupancy => "row.occupancy",
            EventsFuture => "events.future",
            EventsCurrent => "events.current",
            DeckCivilIdentity => "deck.civil_identity",
            DeckMilitaryDiscard => "deck.military_discard",
        }
    }
}

/// A numeric field on a rival's [`PlayerState`] a scalar probe bumps.
/// Mirrors the closures `_bump("...", delta)` built in Python; an enum here
/// instead of a stringly-typed attribute name so [`bump_rival_field`]'s
/// `match` is exhaustive against the struct.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RivalField {
    Culture,
    CultureRateExtra,
    ScienceRateExtra,
    StrengthExtra,
    Science,
    Food,
    Resources,
    CivilActions,
    MilitaryActions,
    WorkersFree,
    YellowBank,
    HappyExtra,
}

/// What a probe does to a cloned trial position. Mirrors `Probe.mutate`:
/// Python closes a closure over the perturbation, this closes a `match` arm
/// in [`Mutation::apply`] instead -- an enum reads more naturally here than a
/// trait because there is no per-mutation state beyond the bump amount, and
/// an enum keeps [`PROBES`] a plain `const` table.
#[derive(Clone, Copy, Debug)]
enum Mutation {
    Bump(RivalField, i32),
    SwapRow,
    ReverseRow,
    ClearRow,
    RivalHandIds,
    RivalHandCivilSize,
    RivalHandMilitarySize,
    RivalTechs,
    RivalWonders,
    RivalColonies,
    RivalWonderProgress,
    RivalGovernment,
    ClearFutureEvents,
    ClearCurrentEvents,
    MaskCivilDeck,
    ClearMilDiscard,
}

impl Mutation {
    fn apply(self, state: &mut GameState, seat: u8) -> Result<(), String> {
        match self {
            Mutation::Bump(field, delta) => {
                bump_rival_field(state, seat, field, delta);
                Ok(())
            }
            Mutation::SwapRow => mutate_swap_row(state, seat),
            Mutation::ReverseRow => mutate_reverse_row(state),
            Mutation::ClearRow => mutate_clear_row(state),
            Mutation::RivalHandIds => mutate_rival_hand_ids(state, seat),
            Mutation::RivalHandCivilSize => {
                for p in rivals_mut(state, seat) {
                    p.hidden_civil = p.hidden_civil.saturating_add(1);
                }
                Ok(())
            }
            Mutation::RivalHandMilitarySize => {
                for p in rivals_mut(state, seat) {
                    p.hand_military.pop();
                }
                Ok(())
            }
            Mutation::RivalTechs => mutate_rival_techs(state, seat),
            Mutation::RivalWonders => {
                for p in rivals_mut(state, seat) {
                    p.completed_wonders.pop();
                }
                Ok(())
            }
            Mutation::RivalColonies => {
                for p in rivals_mut(state, seat) {
                    p.colonies.pop();
                }
                Ok(())
            }
            Mutation::RivalWonderProgress => {
                for p in rivals_mut(state, seat) {
                    p.wonder = CardId::NONE;
                    p.wonder_steps = 0;
                }
                Ok(())
            }
            Mutation::RivalGovernment => mutate_rival_government(state, seat),
            Mutation::ClearFutureEvents => {
                state.future_events = CardList::new();
                state.seeded_by = [NOT_SEEDED; crate::NUM_CARDS];
                Ok(())
            }
            Mutation::ClearCurrentEvents => {
                state.current_events = CardList::new();
                Ok(())
            }
            Mutation::MaskCivilDeck => {
                if !state.civil_deck.is_empty() {
                    let first = state.civil_deck.as_slice()[0];
                    for c in state.civil_deck.as_mut_slice() {
                        *c = first;
                    }
                }
                Ok(())
            }
            Mutation::ClearMilDiscard => {
                for pile in state.discarded_military.iter_mut() {
                    *pile = CardList::new();
                }
                Ok(())
            }
        }
    }
}

fn rivals_mut(state: &mut GameState, seat: u8) -> impl Iterator<Item = &mut PlayerState> {
    let n = state.num_players as usize;
    state.players[..n].iter_mut().filter(move |p| p.idx != seat && !p.resigned)
}

fn bump_rival_field(state: &mut GameState, seat: u8, field: RivalField, delta: i32) {
    for p in rivals_mut(state, seat) {
        match field {
            RivalField::Culture => p.culture = (p.culture as i32 + delta).max(0) as u16,
            RivalField::CultureRateExtra => p.culture_rate_extra = (p.culture_rate_extra as i32 + delta) as i16,
            RivalField::ScienceRateExtra => p.science_rate_extra = (p.science_rate_extra as i32 + delta) as i16,
            RivalField::StrengthExtra => p.strength_extra = (p.strength_extra as i32 + delta) as i16,
            RivalField::Science => p.science = (p.science as i32 + delta).max(0) as u16,
            RivalField::Food => p.food = (p.food as i32 + delta).max(0) as u16,
            RivalField::Resources => p.resources = (p.resources as i32 + delta).max(0) as u16,
            RivalField::CivilActions => {
                p.civil_actions = (p.civil_actions as i32 + delta).clamp(i8::MIN as i32, i8::MAX as i32) as i8
            }
            RivalField::MilitaryActions => {
                p.military_actions = (p.military_actions as i32 + delta).clamp(i8::MIN as i32, i8::MAX as i32) as i8
            }
            RivalField::WorkersFree => p.workers_free = (p.workers_free as i32 + delta).max(0) as u8,
            RivalField::YellowBank => p.yellow_bank = (p.yellow_bank as i32 + delta).max(0) as u8,
            RivalField::HappyExtra => p.happy_extra = (p.happy_extra as i32 + delta) as i16,
        }
    }
}

/// Every civil card that can be in an `num_players`-game of this `age`, read
/// straight off the card table -- the same source the engine deals from, so
/// a substituted card is always a card that could really have been there.
/// Mirrors `_civil_pool`'s per-age bucket, minus the multiplicity Python's
/// `civil_deck` (a simulated deck, cards repeated by `count`) carries and
/// immediately dedupes with `sorted(set(names))`: the card table has exactly
/// one row per name already, so there is nothing here to dedupe.
fn civil_pool_for_age(age: Age, num_players: u8) -> Vec<CardId> {
    let idx = (num_players.saturating_sub(2)) as usize;
    let mut ids: Vec<CardId> = CARDS
        .iter()
        .enumerate()
        .filter(|(_, c)| c.age == age && c.kind.is_civil_row() && c.count[idx] > 0)
        .map(|(i, _)| CardId(i as u16))
        .collect();
    ids.sort_by_key(|id| id.name());
    ids
}

/// A different card of the same age as `id`, so costs and legality stay
/// comparable. Mirrors `_other_card`.
fn other_card(id: CardId, num_players: u8) -> Option<CardId> {
    let age = id.get().age;
    civil_pool_for_age(age, num_players).into_iter().find(|&c| c != id)
}

fn mutate_swap_row(state: &mut GameState, _seat: u8) -> Result<(), String> {
    let mut row = state.card_row;
    let mut changed = false;
    for slot in row.iter_mut() {
        if !slot.is_none() {
            if let Some(alt) = other_card(*slot, state.num_players) {
                *slot = alt;
                changed = true;
            }
        }
    }
    if !changed {
        return Err("could not substitute any row card".to_string());
    }
    state.card_row = row;
    Ok(())
}

/// Same cards, reversed -- so every card's civil-action cost changes (slot 0
/// costs 1 CA, the last slot costs 3). Mirrors `_reverse_row`.
fn mutate_reverse_row(state: &mut GameState) -> Result<(), String> {
    let mut cards: Vec<CardId> = state.card_row.iter().copied().filter(|c| !c.is_none()).collect();
    cards.reverse();
    let mut row = state.card_row;
    row.fill(CardId::NONE);
    for (slot, c) in row.iter_mut().zip(cards) {
        *slot = c;
    }
    state.card_row = row;
    Ok(())
}

fn mutate_clear_row(state: &mut GameState) -> Result<(), String> {
    state.card_row.fill(CardId::NONE);
    Ok(())
}

fn mutate_rival_hand_ids(state: &mut GameState, seat: u8) -> Result<(), String> {
    let num_players = state.num_players;
    let mut changed = false;
    for p in rivals_mut(state, seat) {
        let hand: Vec<CardId> = p.hand_civil.as_slice().to_vec();
        let mut new_hand = CardList::new();
        for c in hand {
            let repl = other_card(c, num_players).unwrap_or(c);
            if repl != c {
                changed = true;
            }
            new_hand.push(repl);
        }
        p.hand_civil = new_hand;
    }
    if !changed {
        return Err("no rival civil card could be substituted".to_string());
    }
    Ok(())
}

/// Move a worker off every rival production card -- their board, as opposed
/// to the three rate scalars the app prints on top of it. Mirrors
/// `_rival_techs`.
fn mutate_rival_techs(state: &mut GameState, seat: u8) -> Result<(), String> {
    let n = state.num_players as usize;
    for i in 0..n {
        if state.players[i].idx == seat || state.players[i].resigned {
            continue;
        }
        let ids: Vec<CardId> = state.players[i].techs.iter().map(|(id, _)| id).collect();
        for id in ids {
            if let Some(slot) = state.players[i].techs.get_mut(id) {
                if slot.workers > 0 {
                    slot.workers -= 1;
                    break;
                }
            }
        }
    }
    Ok(())
}

fn mutate_rival_government(state: &mut GameState, seat: u8) -> Result<(), String> {
    let despotism = CardId::by_name("Despotism").ok_or_else(|| "no Despotism card in the table".to_string())?;
    for p in rivals_mut(state, seat) {
        p.government = despotism;
    }
    Ok(())
}

/// One observable a human could read off the app screen. Mirrors `Probe`.
pub struct Probe {
    pub id: ProbeId,
    /// What the operator reads off the screen.
    pub label: &'static str,
    /// The advisor patch syntax, `""` if not askable.
    pub ask: &'static str,
    pub scope: Scope,
    mutation: Mutation,
    /// Rough seconds of human time per round if this turns out mandatory.
    pub seconds: f64,
    /// Some observables are needed for reasons other than evaluation; when
    /// set, the probe is mandatory regardless of its verdict.
    pub always: Option<&'static str>,
}

impl Probe {
    /// Perturb `state` (already a scratch clone) the way a mis-transcription
    /// of this observable would. Mirrors `Probe.mutate`.
    fn mutate(&self, state: &mut GameState, seat: u8) -> Result<(), String> {
        self.mutation.apply(state, seat)
    }
}

macro_rules! probe {
    ($id:ident, $label:expr, $ask:expr, $scope:ident, $mutation:expr, $seconds:expr) => {
        Probe { id: ProbeId::$id, label: $label, ask: $ask, scope: Scope::$scope, mutation: $mutation, seconds: $seconds, always: None }
    };
    ($id:ident, $label:expr, $ask:expr, $scope:ident, $mutation:expr, $seconds:expr, always: $always:expr) => {
        Probe { id: ProbeId::$id, label: $label, ask: $ask, scope: Scope::$scope, mutation: $mutation, seconds: $seconds, always: Some($always) }
    };
}

pub static PROBES: &[Probe] = &[
    // ---- rival scalars the app prints directly on each player panel
    probe!(RivalCulture, "rival culture (the score number)", "p{i} c=", Rival, Mutation::Bump(RivalField::Culture, 25), 2.0,
        always: "the final score, so it is in the result record regardless"),
    probe!(RivalCultureRate, "rival culture per turn", "p{i} cr=", Rival, Mutation::Bump(RivalField::CultureRateExtra, 4), 2.0),
    probe!(RivalScienceRate, "rival science per turn", "p{i} sr=", Rival, Mutation::Bump(RivalField::ScienceRateExtra, 4), 2.0),
    probe!(RivalStrength, "rival military strength", "p{i} str=", Rival, Mutation::Bump(RivalField::StrengthExtra, 8), 2.0),
    probe!(RivalScience, "rival science stock", "p{i} s=", Rival, Mutation::Bump(RivalField::Science, 20), 2.0),
    probe!(RivalFood, "rival food stock", "p{i} f=", Rival, Mutation::Bump(RivalField::Food, 15), 2.0),
    probe!(RivalResources, "rival resource stock", "p{i} r=", Rival, Mutation::Bump(RivalField::Resources, 15), 2.0),
    probe!(RivalCivilActions, "rival civil actions left", "p{i} ca=", Rival, Mutation::Bump(RivalField::CivilActions, 2), 1.5),
    probe!(RivalMilitaryActions, "rival military actions left", "p{i} ma=", Rival, Mutation::Bump(RivalField::MilitaryActions, 2), 1.5),
    probe!(RivalWorkersFree, "rival unused workers", "p{i} fw=", Rival, Mutation::Bump(RivalField::WorkersFree, 3), 1.5),
    probe!(RivalYellowBank, "rival yellow bank", "p{i} yel=", Rival, Mutation::Bump(RivalField::YellowBank, 5), 1.5),
    probe!(RivalHappy, "rival happiness", "p{i} hap=", Rival, Mutation::Bump(RivalField::HappyExtra, 3), 1.5),
    // ---- rival board and hands (the expensive stuff)
    probe!(RivalTechs, "rival tableau: every tech and its workers", "p{i} tech+ <card>:<n>", Rival, Mutation::RivalTechs, 25.0),
    probe!(RivalWonders, "rival completed wonders", "p{i} built+ <wonder>", Rival, Mutation::RivalWonders, 2.5),
    probe!(RivalColonies, "rival colonies", "p{i} colony+ <territory>", Rival, Mutation::RivalColonies, 2.5),
    probe!(RivalWonderProgress, "rival wonder under construction", "p{i} wonder <wonder> <steps>", Rival, Mutation::RivalWonderProgress, 2.5),
    probe!(RivalGovernment, "rival government", "p{i} gov=", Rival, Mutation::RivalGovernment, 2.0),
    probe!(RivalHandCivilSize, "rival civil hand size", "p{i} hc=", Rival, Mutation::RivalHandCivilSize, 2.0),
    probe!(RivalHandCivilIds, "rival civil hand CONTENTS (public by the rules)", "p{i} hand <card>, <card>", Rival, Mutation::RivalHandIds, 8.0),
    probe!(RivalHandMilitarySize, "rival military hand size", "p{i} hm=", Rival, Mutation::RivalHandMilitarySize, 2.0),
    // ---- shared board
    probe!(RowContents, "which cards are in the card row", "deal <card> <card>", Shared, Mutation::SwapRow, 13.0,
        always: "the bot cannot take a card the mirror does not have"),
    probe!(RowOrder, "the LEFT-TO-RIGHT ORDER of the row (= CA cost)", "row <card>, ...", Shared, Mutation::ReverseRow, 4.0),
    probe!(RowOccupancy, "which row slots are empty", "row ...", Shared, Mutation::ClearRow, 2.0,
        always: "legality of every take"),
    probe!(EventsFuture, "the face-down politics deck", "(not askable: hidden)", Shared, Mutation::ClearFutureEvents, 0.0),
    probe!(EventsCurrent, "the current events deck", "event <card>", Shared, Mutation::ClearCurrentEvents, 3.0),
    probe!(DeckCivilIdentity, "which civil cards are left in the deck", "(not askable: hidden)", Shared, Mutation::MaskCivilDeck, 0.0),
    probe!(DeckMilitaryDiscard, "the military discard pile", "(not askable in the app)", Shared, Mutation::ClearMilDiscard, 0.0),
];

// --------------------------------------------------------------- the probe

/// Exactly `advisor::rank_moves`'s scoring path, as `(move, score)` pairs.
/// Kept in lockstep with it deliberately: if the two diverge, the harness
/// would be deriving its field list from a bot other than the one in the
/// room. Mirrors `_score_moves`.
fn score_moves(state: &GameState, weights: &Weights) -> Vec<(Move, f64)> {
    let moves = legal::legal_moves(state);
    if moves.is_empty() {
        return Vec::new();
    }
    let idx = state.decider();
    let non_resign: Vec<Move> = moves.as_slice().iter().copied().filter(|m| !matches!(m, Move::Resign)).collect();
    let candidates: &[Move] = if non_resign.is_empty() { moves.as_slice() } else { &non_resign };
    let ctx = rivals::rival_context(state, idx, None, None);
    let end_bias = weights.get(WeightKey::EndTurnBias);
    let mut out = Vec::with_capacity(candidates.len());
    for &mv in candidates {
        let mut trial = state.clone();
        apply::apply(&mut trial, mv);
        let mut val = eval::evaluate(&trial, idx, weights, Some(&ctx), None);
        if matches!(mv, Move::EndTurn) {
            val += end_bias;
        }
        out.push((mv, val));
    }
    out
}

fn same_move_set(a: &[(Move, f64)], b: &[(Move, f64)]) -> bool {
    a.len() == b.len() && a.iter().all(|(m, _)| b.iter().any(|(m2, _)| m2 == m))
}

fn ranked_moves(xs: &[(Move, f64)]) -> Vec<Move> {
    let mut v = xs.to_vec();
    // A stable sort, exactly like Python's `sorted`: two moves scored equal
    // keep the order `score_moves` generated them in, which is what makes
    // comparing `order_b` to `order_a` mean "the ranking changed" rather
    // than "an arbitrary tie broke differently".
    v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    v.into_iter().map(|(m, _)| m).collect()
}

/// Mirrors `_verdict`.
fn verdict(base: &[(Move, f64)], after: &[(Move, f64)]) -> Verdict {
    if base.is_empty() || after.is_empty() {
        return Verdict::Inert;
    }
    if !same_move_set(base, after) {
        // the perturbation changed which moves exist at all: a rules-level
        // dependency, not an evaluation one
        return Verdict::Legal;
    }
    let byte_identical = base.iter().all(|(m, v)| {
        let av = after.iter().find(|(m2, _)| m2 == m).map(|&(_, s)| s).unwrap_or(f64::NAN);
        (v - av).abs() < 1e-12
    });
    if byte_identical {
        return Verdict::Inert;
    }
    let ob = ranked_moves(base);
    let oa = ranked_moves(after);
    if ob[0] != oa[0] {
        return Verdict::Move;
    }
    if ob != oa {
        return Verdict::Rank;
    }
    Verdict::Score
}

/// Classify every probe against one position. Mirrors `probe_position`.
pub fn probe_position(state: &GameState, seat: u8, weights: &Weights, probes: &[Probe]) -> Vec<(ProbeId, Verdict)> {
    let base = score_moves(state, weights);
    probes
        .iter()
        .map(|pr| {
            let mut trial = state.clone();
            let v = match pr.mutate(&mut trial, seat) {
                // A probe that cannot be applied to this position tells us
                // nothing. Erring toward `Inert` would silently stop the
                // harness asking for something the bot reads, so err the
                // other way: the operator keeps typing it and the bug is
                // visible instead of invisible. Mirrors the `except
                // Exception: out[pr.id] = RANK` branch.
                Err(_) => Verdict::Rank,
                Ok(()) => verdict(&base, &score_moves(&trial, weights)),
            };
            (pr.id, v)
        })
        .collect()
}

/// Union over several positions: the strongest verdict each probe earns.
/// Mirrors `probe_positions`.
pub fn probe_positions(positions: &[GameState], seat: u8, weights: &Weights, probes: &[Probe]) -> Vec<(ProbeId, Verdict)> {
    let mut out: Vec<(ProbeId, Verdict)> = probes.iter().map(|p| (p.id, Verdict::Inert)).collect();
    for st in positions {
        for (pid, v) in probe_position(st, seat, weights, probes) {
            if let Some(entry) = out.iter_mut().find(|(id, _)| *id == pid) {
                entry.1 = entry.1.worse(v);
            }
        }
    }
    out
}

/// One row of the operator-facing report: a probe, the verdict it earned,
/// and why. Mirrors `Requirement`.
pub struct Requirement {
    pub probe: &'static Probe,
    pub verdict: Verdict,
    pub reason: String,
}

impl Requirement {
    pub fn mandatory(&self) -> bool {
        self.verdict.is_mandatory() || self.probe.always.is_some()
    }

    pub fn to_json(&self) -> crate::fixtures::Json {
        use crate::fixtures::Json;
        Json::obj(vec![
            ("id", Json::Str(self.probe.id.as_str().to_string())),
            ("verdict", Json::Str(self.verdict.as_str().to_string())),
            ("mandatory", Json::Bool(self.mandatory())),
            ("label", Json::Str(self.probe.label.to_string())),
            ("ask", Json::Str(self.probe.ask.to_string())),
            ("scope", Json::Str(if self.probe.scope == Scope::Rival { "rival".to_string() } else { "shared".to_string() })),
            ("reason", Json::Str(self.reason.clone())),
        ])
    }
}

/// Turn raw verdicts into the operator-facing list, mandatory first. Mirrors
/// `requirements`.
pub fn requirements(verdicts: &[(ProbeId, Verdict)], probes: &'static [Probe]) -> Vec<Requirement> {
    let mut out: Vec<Requirement> = probes
        .iter()
        .map(|pr| {
            let v = verdicts.iter().find(|(id, _)| *id == pr.id).map(|&(_, v)| v).unwrap_or(Verdict::Inert);
            let reason = if let Some(always) = pr.always {
                if !v.is_mandatory() {
                    format!("not read by the evaluator, but required: {always}")
                } else {
                    reason_for(v)
                }
            } else {
                reason_for(v)
            };
            Requirement { probe: pr, verdict: v, reason }
        })
        .collect();
    out.sort_by(|a, b| {
        (!a.mandatory())
            .cmp(&!b.mandatory())
            .then_with(|| b.verdict.cmp(&a.verdict))
            .then_with(|| a.probe.id.as_str().cmp(b.probe.id.as_str()))
    });
    out
}

fn reason_for(v: Verdict) -> String {
    match v {
        Verdict::Legal => "changes which moves are legal at all (rules, not eval)".to_string(),
        Verdict::Move => "changes the move the bot plays".to_string(),
        Verdict::Rank => "changes the ranking below the top move".to_string(),
        Verdict::Score => "shifts scores but never the ranking (advisory)".to_string(),
        Verdict::Inert => "the evaluator is blind to it -- do not transcribe".to_string(),
    }
}

/// Seconds of human time per round implied by a requirement list. Mirrors
/// `transcription_cost`.
pub fn transcription_cost(reqs: &[Requirement], num_rivals: u32) -> f64 {
    reqs.iter()
        .filter(|r| r.mandatory())
        .map(|r| r.probe.seconds * if r.probe.scope == Scope::Rival { num_rivals as f64 } else { 1.0 })
        .sum()
}

// -------------------------------------------------- positions to probe with

/// A handful of real mid-game positions for `seat`, from cheap self-play.
/// Used for the pre-flight report (`harness fields`). In a live session
/// `harness::play` probes the actual board instead, which is both free and
/// more honest. Mirrors `sample_positions`.
pub fn sample_positions(num_players: u8, seat: u8, count: usize, seed: u64, weights: &Weights) -> Vec<GameState> {
    use crate::bots::weighted::eval::WeightedBot;
    let bot = WeightedBot::new(*weights);
    let mut st = game::new_game(num_players, seed);
    // Spread the samples across the game: early Age I through late Age III.
    let all_targets = [3u16, 8, 14, 20, 26];
    let mut want: Vec<u16> = all_targets.iter().copied().take(count).collect();
    if want.is_empty() {
        want.push(8);
    }
    let mut out = Vec::new();
    let mut guard = 0;
    while !st.game_over && guard < 8000 && !want.is_empty() {
        guard += 1;
        if want.contains(&st.round) && st.decider() == seat && st.phase == Phase::Actions {
            out.push(st.clone());
            want.retain(|&r| r != st.round);
        }
        let moves = legal::legal_moves(&st);
        if moves.is_empty() {
            break;
        }
        let mv = bot.choose(&st, moves.as_slice());
        apply::apply(&mut st, mv);
    }
    out
}

/// The pre-flight table: what a human must type, and what they must not.
/// Mirrors `report`.
pub fn report(num_players: u8, seat: u8, weights: Option<Weights>) -> (Vec<Requirement>, f64) {
    let w = weights.unwrap_or_else(|| adv::load_bot(num_players, None).0.weights);
    let positions = sample_positions(num_players, seat, 4, 7, &w);
    let verdicts = probe_positions(&positions, seat, &w, PROBES);
    let reqs = requirements(&verdicts, PROBES);
    let cost = transcription_cost(&reqs, (num_players.saturating_sub(1)) as u32);
    (reqs, cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::test_support::midgame;

    #[test]
    fn worse_returns_the_stronger_of_two_verdicts() {
        assert_eq!(Verdict::Inert.worse(Verdict::Move), Verdict::Move);
        assert_eq!(Verdict::Rank.worse(Verdict::Score), Verdict::Rank);
        assert_eq!(Verdict::Score.worse(Verdict::Score), Verdict::Score);
    }

    #[test]
    fn move_and_rank_are_mandatory_score_and_inert_are_not() {
        let probe = PROBES.iter().find(|p| p.id == ProbeId::RivalFood).unwrap();
        for v in [Verdict::Move, Verdict::Rank] {
            let req = Requirement { probe, verdict: v, reason: String::new() };
            assert!(req.mandatory(), "{v:?}");
        }
        for v in [Verdict::Score, Verdict::Inert] {
            let req = Requirement { probe, verdict: v, reason: String::new() };
            assert!(!req.mandatory(), "{v:?}");
        }
    }

    /// Some things are needed for legality or for the result record even
    /// when the evaluator is blind to them.
    #[test]
    fn always_fields_are_mandatory_whatever_the_verdict() {
        let probe = PROBES.iter().find(|p| p.id == ProbeId::RowContents).unwrap();
        assert!(probe.always.is_some());
        let req = Requirement { probe, verdict: Verdict::Inert, reason: String::new() };
        assert!(req.mandatory());
    }

    fn zero_weights() -> Weights {
        let mut w = Weights::defaults();
        for &k in WeightKey::ALL {
            w.set(k, 0.0);
        }
        w
    }

    /// A bot that values nothing must be told to transcribe nothing. With an
    /// all-zero weight vector every position scores 0.0, so no perturbation
    /// can change an EVALUATION -- and the derivation must say so rather
    /// than fall back on a list somebody typed. Legality is separate and
    /// allowed to survive.
    #[test]
    fn a_bot_that_evaluates_nothing_is_told_to_transcribe_nothing() {
        let board = midgame(3, 0, 5, 8);
        let w = zero_weights();
        let v = probe_position(&board.state, board.me, &w, PROBES);
        let read: Vec<ProbeId> = v.iter().filter(|(_, ver)| ver.is_evaluated()).map(|(id, _)| *id).collect();
        assert!(read.is_empty(), "an eval that reads nothing still claimed to read {read:?}");
    }

    #[test]
    fn a_real_weight_vector_demands_something() {
        let board = midgame(3, 0, 5, 8);
        let v = probe_position(&board.state, board.me, &Weights::defaults(), PROBES);
        assert!(v.iter().any(|(_, ver)| ver.is_evaluated()), "no observable at all reached the evaluation");
    }

    /// An INERT verdict is a strong claim; check one directly against the
    /// raw scoring path rather than trusting `probe_position` about itself.
    #[test]
    fn inert_means_byte_identical_scores() {
        let board = midgame(3, 0, 5, 8);
        let w = Weights::defaults();
        let v = probe_position(&board.state, board.me, &w, PROBES);
        let inert: Vec<&Probe> = PROBES.iter().filter(|p| v.iter().any(|(id, ver)| *id == p.id && *ver == Verdict::Inert)).collect();
        assert!(!inert.is_empty());
        let base = score_moves(&board.state, &w);
        for p in inert {
            let mut trial = board.state.clone();
            p.mutate(&mut trial, board.me).unwrap();
            let after = score_moves(&trial, &w);
            assert!(same_move_set(&base, &after), "{}", p.id.as_str());
            for &(mv, val) in &base {
                let av = after.iter().find(|(m, _)| *m == mv).unwrap().1;
                assert_eq!(val, av, "{}", p.id.as_str());
            }
        }
    }

    /// Probes mutate copies. If one leaked, the live game would be
    /// corrupted by the very code meant to protect it.
    #[test]
    fn probing_is_side_effect_free() {
        let board = midgame(3, 0, 5, 8);
        let before = legal::legal_moves(&board.state);
        let culture: Vec<u16> = board.state.players[..board.state.num_players as usize].iter().map(|p| p.culture).collect();
        let row = board.state.card_row;
        let _ = probe_position(&board.state, board.me, &Weights::defaults(), PROBES);
        assert_eq!(legal::legal_moves(&board.state).as_slice(), before.as_slice());
        let culture_after: Vec<u16> = board.state.players[..board.state.num_players as usize].iter().map(|p| p.culture).collect();
        assert_eq!(culture_after, culture);
        assert_eq!(board.state.card_row, row);
    }

    /// A probe whose mutation errors out must not be mistaken for "nothing
    /// to type". `row.contents`'s real mutation (`mutate_swap_row`) fails
    /// exactly this way on an empty row: nothing to substitute.
    #[test]
    fn an_unapplicable_probe_never_reads_as_inert() {
        let mut state = game::new_game(3, 2);
        state.card_row.fill(CardId::NONE);
        let probe = PROBES.iter().find(|p| p.id == ProbeId::RowContents).unwrap();
        let v = probe_position(&state, 0, &Weights::defaults(), std::slice::from_ref(probe));
        assert_ne!(v[0].1, Verdict::Inert);
    }

    #[test]
    fn cost_only_counts_mandatory_and_scales_by_rivals() {
        let cr = PROBES.iter().find(|p| p.id == ProbeId::RivalCultureRate).unwrap();
        let ro = PROBES.iter().find(|p| p.id == ProbeId::RowOrder).unwrap();
        let rf = PROBES.iter().find(|p| p.id == ProbeId::RivalFood).unwrap();
        let reqs = vec![
            Requirement { probe: cr, verdict: Verdict::Move, reason: String::new() },
            Requirement { probe: ro, verdict: Verdict::Move, reason: String::new() },
            Requirement { probe: rf, verdict: Verdict::Inert, reason: String::new() },
        ];
        let two = transcription_cost(&reqs, 2);
        let one = transcription_cost(&reqs, 1);
        assert!(two > one);
        assert_eq!(two - one, cr.seconds);
    }

    #[test]
    fn requirements_serialise_with_the_expected_keys() {
        let board = midgame(3, 0, 5, 8);
        let v = probe_position(&board.state, board.me, &Weights::defaults(), PROBES);
        for r in requirements(&v, PROBES) {
            let j = r.to_json();
            for key in ["id", "verdict", "mandatory", "ask"] {
                assert!(j.get(key).is_some(), "missing {key}");
            }
        }
    }

    /// When the evaluator gains an eye, the mandatory set must grow by
    /// itself.
    #[test]
    fn growing_verdicts_grow_the_mandatory_set() {
        let blind: Vec<(ProbeId, Verdict)> = PROBES.iter().map(|p| (p.id, Verdict::Inert)).collect();
        let mut sighted = blind.clone();
        for (id, v) in sighted.iter_mut() {
            if *id == ProbeId::RowOrder {
                *v = Verdict::Move;
            }
            if *id == ProbeId::RivalHandCivilIds {
                *v = Verdict::Rank;
            }
        }
        let before: Vec<ProbeId> = requirements(&blind, PROBES).into_iter().filter(|r| r.mandatory()).map(|r| r.probe.id).collect();
        let after: Vec<ProbeId> = requirements(&sighted, PROBES).into_iter().filter(|r| r.mandatory()).map(|r| r.probe.id).collect();
        let grew: Vec<ProbeId> = after.into_iter().filter(|id| !before.contains(id)).collect();
        assert_eq!(grew, vec![ProbeId::RowOrder, ProbeId::RivalHandCivilIds]);
    }

    #[test]
    fn union_over_positions_takes_the_strongest_verdict() {
        let mut merged = Verdict::Inert;
        for v in [Verdict::Inert, Verdict::Score, Verdict::Move, Verdict::Inert] {
            merged = merged.worse(v);
        }
        assert_eq!(merged, Verdict::Move);
    }

    /// The clamps on strength make it nonlinear, so unlike rival culture it
    /// can genuinely move the argmax. Sample a wide grid and stop at the
    /// first hit, matching the Python test's own note that this is a
    /// sampling property, not a guaranteed one on any single position.
    #[test]
    fn rival_strength_is_sometimes_decision_relevant() {
        let w = Weights::defaults();
        let mut seen = Vec::new();
        'outer: for seed in [5u64, 6, 7] {
            for stop in [5u16, 8, 11, 14, 17, 20] {
                let b = midgame(3, 0, seed, stop);
                let v = probe_position(&b.state, b.me, &w, PROBES);
                let ver = v.iter().find(|(id, _)| *id == ProbeId::RivalStrength).unwrap().1;
                seen.push(ver);
                if ver.is_mandatory() {
                    break 'outer;
                }
            }
        }
        assert!(seen.iter().any(|v| *v != Verdict::Inert), "rival strength never reached the evaluation");
        assert!(seen.iter().any(|v| v.is_mandatory()), "rival strength never changed a decision: {seen:?}");
    }

    /// `rival.techs` is the single most expensive thing a human could type.
    /// If mirroring the whole tableau ever changed the RECOMMENDED MOVE
    /// (verdict `Legal`/`Move`), the per-game cost roughly doubles. The bar
    /// is `Rank`, not mandatory-in-general: reordering the tail of the
    /// candidate list is an established property of this probe.
    #[test]
    fn the_expensive_opponent_board_never_changes_the_top_recommendation() {
        let w = Weights::defaults();
        let mut worst = Verdict::Inert;
        for stop in [5u16, 8, 11, 14, 17, 20, 23] {
            let b = midgame(3, 0, 5, stop);
            let v = probe_position(&b.state, b.me, &w, PROBES);
            let ver = v.iter().find(|(id, _)| *id == ProbeId::RivalTechs).unwrap().1;
            worst = worst.worse(ver);
        }
        assert!(!matches!(worst, Verdict::Legal | Verdict::Move), "{worst:?}");
    }
}
