//! The variant roster: a pool of distinct, strong, rule-based opponents.
//!
//! Ports `engine/bots/variants/` (`base.py` + `culture.py`/`military.py`/
//! `science.py`/`wonder.py`/`infrastructure.py`/`tempo.py`). Read
//! `engine/bots/variants/base.py`'s own module doc comment first -- it is
//! the design rationale (self-play converges to an opponent that only beats
//! itself; this roster is six structurally different, hand-written, human-
//! cited strategies to train against instead). Short version restated here:
//! every variant is [`bots::book::BookBot`]'s v2 rule list (the empirical
//! tournament tier list) with a per-archetype knob [`Profile`] bolted on,
//! and (for some archetypes) a reordered rule list -- "what do you do with
//! your civil action when several rules fire" *is* the strategy.
//!
//! ## Shape: no inheritance, one generic engine plus six data tables
//!
//! Python's `VariantBot` is a `BookBot` subclass; each archetype
//! (`CultureBot`, `MilitaryBot`, ...) is a further subclass overriding a
//! `PROFILE` dict and occasionally a method (`MilitaryBot.mil_goal`,
//! `MilitaryBot._r_tactics`, `ScienceBot._best_build`). Rust has no
//! inheritance, so this port does not attempt one class hierarchy per
//! archetype. Instead:
//!
//! * [`Profile`] is one `const`-friendly struct covering every knob in
//!   Python's `DEFAULT_PROFILE`, plus three fields with no Python
//!   equivalent name but real Python behaviour behind them --
//!   [`Profile::econ_first_until_age`]/[`Profile::age_strength_floor`]
//!   (`MilitaryBot.mil_goal`'s override) and [`Profile::science_ceiling`]
//!   (`ScienceBot._best_build`'s override) -- turned from "a method this one
//!   subclass overrides" into "a knob this one profile sets and the others
//!   leave at the default", which is what they always were in substance.
//!   [`Profile::agg_order`] does the same for `MilitaryBot._politics`'s only
//!   real deviation from the shared politics rule (§ below).
//! * `culture.rs`/`military.rs`/`science.rs`/`wonder.rs`/`infrastructure.rs`/
//!   `tempo.rs` each export one `const PROFILE: Profile` and one
//!   `const RULES: &[RuleId]` -- literally the diff of that Python file
//!   against `DEFAULT_PROFILE`/`VariantBot.RULES`, restated as data. Every
//!   rule function in this file (`r_play_leader`, `best_take`, `mil_goal`,
//!   `politics`, ...) reads a `&Profile` parameter; none of them know or
//!   care which archetype's profile they were handed.
//! * [`Archetype`] is the seven-way (six archetypes; [`VariantBot`] itself
//!   has no "generic" variant, matching Python having no instantiable bare
//!   `VariantBot`) closed enum a caller actually holds, `match`ed to select
//!   a `Profile`/`RULES` pair -- this is the one place archetype identity
//!   exists as code rather than data, and it is an exhaustive `match`, not a
//!   string.
//!
//! ## Pending decisions: delegated to `BookBot`, unchanged, on purpose
//!
//! No Python variant overrides `_pending`/`_choice`/`_auction`/`_defense`/
//! `_colonize` -- auctions, aggression defence and colonization forces are
//! played identically by every archetype and by plain `BookBot`. This port
//! does the same by construction rather than by omission: [`VariantBot::
//! choose`] calls [`BookBot::pending_pick`] directly (widened to
//! `pub(crate)` in `book.rs` for exactly this) whenever `state.pending` is
//! non-empty, before an [`Archetype`] is even consulted.
//!
//! ## Legality: every read below is public
//!
//! Every function in this module that inspects a rival reads only public
//! board state: [`crate::effects::state_stats`] (computed from a player's
//! own played tableau/army, visible to every player at the table in Through
//! the Ages), [`crate::combat::attack_strength`] (the same, plus public pact
//! state), `PlayerState::culture`/`government`/`wonder`/`completed_wonders`/
//! `techs` (the physical board), and `state.card_row` (the shared row).
//! `p.hand_civil`/`p.hand_military` are read only for `p` itself (the
//! deciding player's own hand -- legitimate self-knowledge, e.g.
//! [`leader_rank`]'s Leonardo/Columbus conditions and [`best_take`]'s hand-
//! size penalty), mirroring [`book::leader_rank`]'s identical restriction.
//! Nothing here reads a rival's `hand_civil`/`hand_military`, and nothing
//! iterates `state.civil_deck`/`state.military_deck` at all (their true,
//! shuffled order is exactly the information `bots::counting`'s module doc
//! comment says a legal bot must never read).
use crate::bots::book::{self, BookBot, Ctx, V2Tunables};
use crate::cards::{Age, CardId, CardType, Production};
use crate::combat;
use crate::costs;
use crate::economy;
use crate::effects;
use crate::moves::{Move, PactSide};
use crate::state::{GameState, Phase, PlayerState};

pub mod culture;
pub mod infrastructure;
pub mod military;
pub mod science;
pub mod tempo;
pub mod wonder;

// ------------------------------------------------------------- knob tables

/// A knob that may take a different value at 2/3/4 players (Python's
/// `pc(value, nplayers)`: an int-keyed dict resolved by table size, falling
/// back to the highest key present for any other count). Every base-game
/// table has 2-4 players, so this is exactly those three slots -- no
/// fallback branch is reachable from a real game, unlike Python's `pc()`,
/// which is also called (harmlessly) on non-int-keyed dicts it must pass
/// through unchanged; that ambiguity does not exist here because a
/// `Pc<T>` is never anything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pc<T: Copy> {
    pub p2: T,
    pub p3: T,
    pub p4: T,
}

impl<T: Copy> Pc<T> {
    /// The common case: the same value at every table size.
    pub(crate) const fn flat(v: T) -> Pc<T> {
        Pc { p2: v, p3: v, p4: v }
    }

    pub(crate) const fn resolve(&self, nplayers: u8) -> T {
        match nplayers {
            2 => self.p2,
            3 => self.p3,
            _ => self.p4,
        }
    }
}

/// Multipliers on [`prod_value`]'s six axes. Mirrors Python's
/// `PROFILE["prod_weights"]` dict; every archetype overrides only the axes
/// its own strategy actually disagrees about (e.g. `CultureBot` only touches
/// `culture`/`happy`/`resources`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProdWeights {
    pub food: Pc<f64>,
    pub resources: Pc<f64>,
    pub science: Pc<f64>,
    pub culture: Pc<f64>,
    pub happy: Pc<f64>,
    pub strength: Pc<f64>,
}

const DEFAULT_PROD_WEIGHTS: ProdWeights = ProdWeights {
    food: Pc::flat(1.0),
    resources: Pc::flat(1.0),
    science: Pc::flat(1.0),
    culture: Pc::flat(1.0),
    happy: Pc::flat(1.0),
    strength: Pc::flat(1.0),
};

/// The strength this variant is trying to reach, before any
/// [`Profile::mil_margin`]/[`Profile::age_strength_floor`] adjustment. Mirrors
/// Python's `mil_stance` string knob (`"floor"`/`"top2"`/`"aggro"`); no
/// archetype in this roster actually sets `"aggro"` (it computes the same
/// target as `"top2"`, only a bigger `mil_margin` -- see `mil_goal`'s Python
/// docstring), but the variant is kept so the enum matches the source knob's
/// full domain rather than silently narrowing it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum MilStance {
    /// "Never be the weakest": match `ctx.mil_target` (BookBot's own floor).
    Floor,
    /// "Top 2 military position": match the strongest rival.
    Top2,
    /// Same target as `Top2`; the aggression this stance is named for comes
    /// from a bigger `mil_margin`, not a different target formula. Unused by
    /// every current archetype (none of the six ported profiles set it --
    /// `MilitaryBot` uses `Top2`), kept so this enum matches the Python
    /// knob's full domain rather than silently narrowing it; a future
    /// archetype may want it.
    #[allow(dead_code)]
    Aggro,
}

/// One archetype's full knob set. Mirrors `DEFAULT_PROFILE` merged with a
/// `PROFILE` dict override in Python -- but is not itself the *merge*
/// (Rust has no dict update); each archetype module builds its own `const
/// PROFILE: Profile = Profile { field: ..., ..DEFAULT_PROFILE };`, which is
/// the direct spelling of `dict(DEFAULT_PROFILE); prof.update(PROFILE)`.
pub(crate) struct Profile {
    pub prod_weights: ProdWeights,
    /// name -> additive bonus, applied when developing AND when taking.
    pub tech_bonus: &'static [(&'static str, Pc<f64>)],
    /// Names never developed and never taken (the genuine "I do not buy this
    /// line at all" calls, e.g. Tempo skipping Iron).
    pub tech_veto: &'static [(&'static str, Pc<bool>)],
    /// name -> additive bonus applied only when taking from the row.
    pub card_bonus: &'static [(&'static str, Pc<f64>)],
    pub leader_bonus: &'static [(&'static str, Pc<f64>)],
    /// name -> MULTIPLIER (default 1.0, not 0.0 -- see [`wonder_value`]).
    pub wonder_bonus: &'static [(&'static str, Pc<f64>)],
    pub wonder_appetite: f64,
    pub wonder_max: u8,
    /// Multiplier on the convex CA price ladder ([`book::v2_price_ladder`]).
    pub price_scale: f64,
    /// Hard cap on the slot price this variant will pay, by age index
    /// (0=A, 1=I, 2=II, 3=III, 4=IV).
    pub max_take_cost: [i32; 5],
    /// The short universal 3-CA list -- cards worth reaching into the
    /// expensive slots for regardless of `max_take_cost`/hand-size gating.
    pub must_buy_3ca: &'static [&'static str],
    pub three_ca_min_actions: i32,
    pub hand_penalty: f64,
    pub mil_stance: MilStance,
    pub mil_margin: Pc<i32>,
    /// Extra strength lead demanded before firing an aggression, by age.
    pub agg_lead: [i32; 5],
    pub war_lead: i32,
    pub war_from_age: u8,
    /// "Certainly don't play [events] when you are the weakest player."
    pub seed_events_when_weakest: bool,
    /// How many military units this variant is willing to staff, by age.
    pub unit_cap: [i32; 5],
    /// Card TYPE -> additive bonus when placing a worker.
    pub build_bonus: &'static [(CardType, f64)],
    pub pop_appetite: f64,
    /// Minimum government value before burning a whole turn on a revolution.
    pub revolution_min: f64,
    /// Upgrade TYPEs this variant refuses to spend a civil action upgrading
    /// in place (the Iron-vs-Bronze disagreement lives here).
    pub upgrade_veto: &'static [(CardType, Pc<bool>)],
    /// `MilitaryBot.mil_goal`'s economy-first gate: while the age is at or
    /// below this and food/resources are short, hold to the plain
    /// `ctx.mil_target` floor instead of `mil_stance`'s target. `None` for
    /// every other archetype (no gate).
    pub econ_first_until_age: Option<u8>,
    /// `MilitaryBot.AGE_STRENGTH_FLOOR`: an absolute strength floor by age,
    /// on top of whatever `mil_stance` computes. `None` for every other
    /// archetype.
    pub age_strength_floor: Option<[i32; 5]>,
    /// `ScienceBot.SCIENCE_CEILING`: once current science production
    /// reaches this (by age, player-count-keyed), [`best_build`] stops
    /// adding new lab workers (existing labs may still be upgraded -- that
    /// is a different move kind). `None` for every other archetype.
    pub science_ceiling: Option<[Pc<f64>; 5]>,
    /// `MilitaryBot._AGG_ORDER`: cash-in priority among aggression cards
    /// (resource/science/cube thefts before culture thefts -- flipped once
    /// `ctx.age >= 3`, when "save the culture theft for later" stops
    /// applying). `None` for every other archetype, which breaks aggression
    /// ties on target culture and card name alone, exactly as
    /// [`book::politics`] does.
    pub agg_order: Option<&'static [(&'static str, f64)]>,
}

/// Every knob, with the neutral value that reproduces `BookBot` v2
/// behaviour -- mirrors Python's `DEFAULT_PROFILE`. A diff of an archetype's
/// `PROFILE` against this constant *is* that archetype's strategy.
pub(crate) const DEFAULT_PROFILE: Profile = Profile {
    prod_weights: DEFAULT_PROD_WEIGHTS,
    tech_bonus: &[],
    tech_veto: &[],
    card_bonus: &[],
    leader_bonus: &[],
    wonder_bonus: &[],
    wonder_appetite: 1.0,
    wonder_max: 99,
    price_scale: 1.0,
    max_take_cost: [2, 2, 2, 3, 3],
    must_buy_3ca: &["Air Forces"],
    three_ca_min_actions: 5,
    hand_penalty: 1.0,
    mil_stance: MilStance::Floor,
    mil_margin: Pc::flat(0),
    agg_lead: [4, 4, 3, 3, 3],
    war_lead: 5,
    war_from_age: 1,
    seed_events_when_weakest: true,
    unit_cap: [2, 4, 6, 8, 8],
    build_bonus: &[],
    pop_appetite: 1.0,
    revolution_min: 10.0,
    upgrade_veto: &[],
    econ_first_until_age: None,
    age_strength_floor: None,
    science_ceiling: None,
    agg_order: None,
};

// -------------------------------------------------------------- lookups

fn table_lookup(table: &[(&str, Pc<f64>)], id: CardId, nplayers: u8, default: f64) -> f64 {
    let name = id.name();
    table.iter().find(|&&(n, _)| n == name).map_or(default, |&(_, pc)| pc.resolve(nplayers))
}

fn table_contains(table: &[(&str, Pc<f64>)], id: CardId) -> bool {
    let name = id.name();
    table.iter().any(|&(n, _)| n == name)
}

fn table_bool_contains(table: &[(&str, Pc<bool>)], id: CardId, nplayers: u8) -> bool {
    let name = id.name();
    table.iter().any(|&(n, pc)| n == name && pc.resolve(nplayers))
}

fn table_lookup_type(table: &[(CardType, f64)], typ: CardType, default: f64) -> f64 {
    table.iter().find(|&&(t, _)| t == typ).map_or(default, |&(_, v)| v)
}

fn table_bool_contains_type(table: &[(CardType, Pc<bool>)], typ: CardType, nplayers: u8) -> bool {
    table.iter().any(|&(t, pc)| t == typ && pc.resolve(nplayers))
}

fn table_lookup_str(table: &[(&str, f64)], name: &str, default: f64) -> f64 {
    table.iter().find(|&&(n, _)| n == name).map_or(default, |&(_, v)| v)
}

// ------------------------------------------------------------- valuation

/// Book's phase-dependent production value ([`book::prod_value`]'s formula
/// shape, which is left alone -- food/resources matter early, culture
/// matters late), rescaled per axis by [`Profile::prod_weights`].
pub(crate) fn prod_value(prod: Production, ctx: &Ctx, profile: &Profile) -> f64 {
    let early = ctx.age <= 1;
    let n = ctx.nplayers;
    let w = &profile.prod_weights;
    let mut v = 0.0;
    v += prod.food as f64 * if early { 3.0 } else { 1.5 } * w.food.resolve(n);
    v += prod.resources as f64 * if early { 3.0 } else { 2.0 } * w.resources.resolve(n);
    v += prod.science as f64 * if !ctx.late { 3.5 } else { 1.0 } * w.science.resolve(n);
    v += prod.culture as f64 * if early { 2.0 } else { 4.5 } * w.culture.resolve(n);
    v += prod.happy as f64 * (if ctx.happy_gap >= 0 { 2.5 } else { 1.0 }) * w.happy.resolve(n);
    v += prod.strength as f64 * 1.5 * w.strength.resolve(n);
    v
}

/// Rank, discounted by resource cost ([`book::wonder_value`]), then scaled by
/// this archetype's appetite and per-wonder multiplier.
pub(crate) fn wonder_value(id: CardId, ctx: &Ctx, profile: &Profile) -> f64 {
    let mut v = book::wonder_value(id, ctx);
    v *= profile.wonder_appetite;
    v *= table_lookup(profile.wonder_bonus, id, ctx.nplayers, 1.0);
    v
}

/// The strength this variant is trying to reach. Mirrors `VariantBot.
/// mil_goal`/`MilitaryBot.mil_goal`: `econ_first_until_age`/
/// `age_strength_floor` are `None` for every archetype except Military, so
/// this one function reproduces both Python methods depending only on which
/// `Profile` it is handed -- see this module's top doc comment.
pub(crate) fn mil_goal(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile) -> i32 {
    if let Some(gate_age) = profile.econ_first_until_age {
        if ctx.age <= gate_age && (ctx.food_need > 0 || ctx.res_need > 0) {
            return ctx.mil_target;
        }
    }
    let margin = profile.mil_margin.resolve(ctx.nplayers);
    let mut goal = match profile.mil_stance {
        MilStance::Floor => ctx.mil_target + margin,
        MilStance::Top2 | MilStance::Aggro => {
            let mut top = 0i32;
            let mut any = false;
            for q in state.players[..state.num_players as usize].iter() {
                if q.idx == p.idx || q.resigned {
                    continue;
                }
                let s = effects::state_stats(state, q).strength;
                if !any || s > top {
                    top = s;
                }
                any = true;
            }
            if any {
                top + margin
            } else {
                margin
            }
        }
    };
    if let Some(floor) = profile.age_strength_floor {
        goal = goal.max(floor[ctx.age as usize]);
    }
    goal
}

/// Profile-aware rewrite of [`book::card_value`]: same structure, but every
/// branch can be moved by the profile, which is what makes two variants
/// disagree about the same card row.
pub(crate) fn card_value(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, id: CardId) -> f64 {
    let c = id.get();
    let bonus = table_lookup(profile.tech_bonus, id, ctx.nplayers, 0.0);
    match c.kind {
        CardType::Leader => {
            book::leader_rank(id, ctx, Some(p)) * 1.6 + table_lookup(profile.leader_bonus, id, ctx.nplayers, 0.0)
        }
        CardType::Wonder => wonder_value(id, ctx, profile),
        CardType::Government => book::gov_value(p, ctx, id) + bonus,
        CardType::SpecialTech => {
            let mut base = book::rank_of(book::SPECIAL_RANK, id, 4.0) * 1.5;
            if c.name == "Masonry" && p.wonder.is_none() && p.completed_wonders.is_empty() {
                base *= 0.6; // no wonder to discount yet
            }
            base + bonus
        }
        CardType::Action => book::action_card_value(p, ctx, id) + bonus,
        _ if c.kind.takes_workers() => {
            let mut v = prod_value(c.production, ctx, profile);
            if ctx.version >= 2 && c.name == "Theology" {
                // Selected exactly 0 times in 39 tournament games.
                v *= ctx.tun.theology;
            }
            let lvl = id.level() as i32;
            let best = book::best_level_in(p, c.kind);
            if lvl <= best {
                return 0.0; // never a downgrade
            }
            if c.kind.is_unit() {
                v = c.effects.strength as f64 * 2.0;
                if ctx.strength >= mil_goal(state, p, ctx, profile) {
                    v *= 0.5; // already safe: units are a tax
                }
            }
            if ctx.late && !matches!(c.kind, CardType::Theater | CardType::Temple | CardType::Library) {
                v *= 0.5;
            }
            v + bonus
        }
        CardType::Farm | CardType::Mine | CardType::Lab | CardType::Temple | CardType::Library | CardType::Arena | CardType::Theater | CardType::Infantry | CardType::Cavalry | CardType::Artillery | CardType::Air | CardType::Tactic | CardType::Aggression | CardType::War | CardType::Pact | CardType::Bonus | CardType::Territory | CardType::Event => 1.0,
    }
}

// ------------------------------------------------------------ best-* helpers

/// Choose what to put the free worker on. Adds [`Profile::build_bonus`] and
/// (when [`Profile::science_ceiling`] is set and reached) refuses to staff
/// another lab -- `ScienceBot._best_build`'s override, folded in as data.
pub(crate) fn best_build(
    _state: &GameState,
    _p: &PlayerState,
    ctx: &Ctx,
    profile: &Profile,
    moves: &[Move],
    happy_only: bool,
) -> Option<Move> {
    let block_labs = profile
        .science_ceiling
        .is_some_and(|table| ctx.s.science as f64 >= table[ctx.age as usize].resolve(ctx.nplayers));
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Build { card } = m {
            let typ = card.kind();
            if typ.is_unit() {
                continue;
            }
            if block_labs && typ == CardType::Lab {
                continue;
            }
            let c = card.get();
            let prod = c.production;
            if happy_only && prod.happy == 0 {
                continue;
            }
            let mut v = prod_value(prod, ctx, profile);
            if typ == CardType::Farm {
                v += 3.0 * ctx.food_need.min(2) as f64;
            } else if typ == CardType::Mine {
                v += 3.0 * ctx.res_need.min(2) as f64;
            }
            if prod.happy != 0 && ctx.happy_gap > 0 {
                v += 5.0 * ctx.happy_gap.min(2) as f64;
            }
            v += table_lookup_type(profile.build_bonus, typ, 0.0);
            v -= c.resource_cost as f64 * 0.5;
            if ctx.late && prod.culture == 0 {
                v -= 3.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v > 0.5 {
        best
    } else {
        None
    }
}

pub(crate) fn best_develop(
    state: &GameState,
    p: &PlayerState,
    ctx: &Ctx,
    profile: &Profile,
    moves: &[Move],
    happy_only: bool,
) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Develop { card } = m {
            if table_bool_contains(profile.tech_veto, card, ctx.nplayers) {
                continue;
            }
            let c = card.get();
            if happy_only && c.production.happy == 0 {
                continue;
            }
            let mut v = card_value(state, p, ctx, profile, card);
            let cost = costs::tech_cost(state, p, card).unwrap_or(0);
            v -= cost as f64 * 0.6;
            if c.kind.takes_workers() && p.workers_free == 0 && (c.resource_cost as u16) > p.resources {
                v -= 2.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= 1.0 {
        best
    } else {
        None
    }
}

/// Take cards you will actually play, from the cheap end of the row. See
/// [`book::best_take`]'s doc comment for the shared price-ladder discipline
/// this inherits; every addition here is a [`Profile`] knob.
pub(crate) fn best_take(
    state: &GameState,
    p: &PlayerState,
    ctx: &Ctx,
    profile: &Profile,
    moves: &[Move],
    first_turn: bool,
) -> Option<Move> {
    let hand = p.hand_civil.len() as f64;
    let cap = profile.max_take_cost[ctx.age as usize];
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Take { slot, .. } = m {
            let name = state.card_row[slot as usize];
            if name.is_none() {
                continue;
            }
            if table_bool_contains(profile.tech_veto, name, ctx.nplayers) {
                continue;
            }
            let cost = costs::take_cost(state, p, slot as usize);
            let typ = name.kind();
            if typ == CardType::Wonder && !p.wonder.is_none() {
                continue;
            }
            if typ == CardType::Wonder && p.completed_wonders.len() as u8 >= profile.wonder_max {
                continue;
            }
            if book::V2_NEVER_TAKE.contains(&name.name()) {
                continue;
            }
            // "No leader warrants spending 3 white points (except Age 3)."
            if typ == CardType::Leader && cost >= 3 && ctx.age < 3 {
                continue;
            }
            let has_card_bonus = table_contains(profile.card_bonus, name);
            if (name.name() == "Taj Mahal" || name.name() == "Great Wall") && cost > 1 && !has_card_bonus {
                continue;
            }
            let must = profile.must_buy_3ca.contains(&name.name());
            if cost > cap && !must {
                continue;
            }
            if cost >= 3 && ctx.s.civil_actions < profile.three_ca_min_actions && !must {
                continue;
            }
            let mut v = card_value(state, p, ctx, profile, name);
            v += table_lookup(profile.card_bonus, name, ctx.nplayers, 0.0);
            v -= book::v2_price_ladder(cost) * 3.0 * profile.price_scale;
            v -= hand * profile.hand_penalty;
            if first_turn {
                v += 4.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v > 0.0 {
        best
    } else {
        None
    }
}

// ----------------------------------------------------------------- rules

/// The priority list a caller assembles per archetype -- Python's `RULES`
/// tuple of method-name strings, as an exhaustive enum instead. `Tactics` is
/// only ever listed by [`military::RULES`]: `_r_tactics` is defined once
/// (Python: in `military.py`, the one archetype that plays it), not per
/// archetype, exactly like every other rule here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RuleId {
    Round1,
    Revolution,
    PlayLeader,
    Happiness,
    Tactics,
    MilitaryFloor,
    WonderStep,
    Population,
    PlaceWorker,
    Upgrade,
    Develop,
    ActionCard,
    TakeCard,
}

fn dispatch(rule: RuleId, state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    match rule {
        RuleId::Round1 => r_round1(state, p, ctx, profile, moves),
        RuleId::Revolution => r_revolution(state, p, ctx, profile, moves),
        RuleId::PlayLeader => r_play_leader(state, p, ctx, profile, moves),
        RuleId::Happiness => r_happiness(state, p, ctx, profile, moves),
        RuleId::Tactics => r_tactics(state, p, ctx, profile, moves),
        RuleId::MilitaryFloor => r_military_floor(state, p, ctx, profile, moves),
        RuleId::WonderStep => r_wonder_step(state, p, ctx, profile, moves),
        RuleId::Population => r_population(state, p, ctx, profile, moves),
        RuleId::PlaceWorker => r_place_worker(state, p, ctx, profile, moves),
        RuleId::Upgrade => r_upgrade(state, p, ctx, profile, moves),
        RuleId::Develop => r_develop(state, p, ctx, profile, moves),
        RuleId::ActionCard => r_action_card(state, p, ctx, profile, moves),
        RuleId::TakeCard => r_take_card(state, p, ctx, profile, moves),
    }
}

/// Walk `rules` top to bottom; the first non-`None` result wins.
fn action_phase(
    state: &GameState,
    p: &PlayerState,
    ctx: &Ctx,
    profile: &Profile,
    rules: &[RuleId],
    moves: &[Move],
) -> Move {
    for &r in rules {
        if let Some(mv) = dispatch(r, state, p, ctx, profile, moves) {
            return mv;
        }
    }
    Move::EndTurn
}

/// Round 1: taking cards is the only legal action.
fn r_round1(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if state.round != 1 {
        return None;
    }
    if !moves.iter().any(|m| matches!(m, Move::Take { .. })) {
        return None;
    }
    best_take(state, p, ctx, profile, moves, true)
}

/// Revolution costs the whole turn's civil actions, so it only pays while
/// there is a real gain (`revolution_min`) and many turns left to spend the
/// extra action on.
fn r_revolution(_state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if ctx.late || ctx.age >= 2 {
        return None;
    }
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Revolution { card } = m {
            let v = book::gov_value(p, ctx, card);
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= profile.revolution_min && ctx.rnd <= 8 {
        best
    } else {
        None
    }
}

/// Play a leader immediately -- ranked by [`book::leader_rank`] plus this
/// archetype's [`Profile::leader_bonus`] (e.g. the military variant really
/// does prefer Napoleon; the wonder variant really does prefer Michelangelo).
fn r_play_leader(_state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    let rank = |id: CardId| book::leader_rank(id, ctx, Some(p)) + table_lookup(profile.leader_bonus, id, ctx.nplayers, 0.0);
    let mut best: Option<Move> = None;
    let mut best_key: (f64, &str) = (0.0, "");
    for &m in moves {
        if let Move::PlayLeader { card } = m {
            let key = (rank(card), card.name());
            if best.is_none() || key > best_key {
                best = Some(m);
                best_key = key;
            }
        }
    }
    let (best_mv, best_card) = match best {
        Some(m @ Move::PlayLeader { card }) => (m, card),
        _ => return None,
    };
    if p.leader.is_none() {
        return Some(best_mv);
    }
    // Replacing a leader costs a civil action you do not get back.
    if rank(best_card) >= rank(p.leader) + 2.0 {
        return Some(best_mv);
    }
    None
}

/// Discontent costs a worker every turn and an uprising is a catastrophe;
/// fix it before anything else.
fn r_happiness(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if ctx.happy_gap <= 0 {
        return None;
    }
    best_build(state, p, ctx, profile, moves, true).or_else(|| best_develop(state, p, ctx, profile, moves, true))
}

/// `MilitaryBot._r_tactics`: always take a tactic if you have none (an army
/// with no tactic is worth its raw unit strength and nothing else), but hold
/// an Age II/III tactic -- there is exactly one copy of each -- until Age III
/// or the last round.
fn r_tactics(_state: &GameState, p: &PlayerState, ctx: &Ctx, _profile: &Profile, moves: &[Move]) -> Option<Move> {
    let strength = |id: CardId| id.get().effects.strength;
    let mut any = false;
    let mut best: Option<Move> = None;
    let mut best_key: (i16, &str) = (0, "");
    let mut age1_best: Option<Move> = None;
    let mut age1_key: (i16, &str) = (0, "");
    for &m in moves {
        if let Move::PlayTactic { card } = m {
            any = true;
            let key = (strength(card), card.name());
            if best.is_none() || key > best_key {
                best = Some(m);
                best_key = key;
            }
            if card.get().age == Age::I && (age1_best.is_none() || key > age1_key) {
                age1_best = Some(m);
                age1_key = key;
            }
        }
    }
    if !any {
        return None;
    }
    if p.tactic.is_none() {
        // An Age II tactic played early is a singleton spent early; take an
        // Age I one instead if one is on offer.
        if age1_best.is_some() && ctx.age < 3 {
            return age1_best;
        }
        return best;
    }
    let best_card = match best {
        Some(Move::PlayTactic { card }) => card,
        _ => unreachable!("best was set from a PlayTactic move above"),
    };
    if best_key.0 <= strength(p.tactic) {
        return None;
    }
    let age = best_card.get().age;
    if matches!(age, Age::II | Age::III) && ctx.age < 3 && !ctx.last {
        return None; // hold the singleton
    }
    best
}

/// Never be the weakest civilisation at the table; some variants aim higher
/// (see [`mil_goal`]/[`Profile::mil_stance`]).
fn r_military_floor(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if p.tactic.is_none() {
        let mut best: Option<Move> = None;
        let mut best_v = 0.0;
        for &m in moves {
            if let Move::PlayTactic { card } = m {
                let v = book::rank_of(book::TACTIC_RANK, card, 0.0);
                if best.is_none() || v > best_v {
                    best = Some(m);
                    best_v = v;
                }
            }
        }
        if let Some(m) = best {
            return Some(m);
        }
    }
    if ctx.strength >= mil_goal(state, p, ctx, profile) {
        return None;
    }
    // Upgrade an existing unit first (no new worker needed).
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Upgrade { from, to } = m {
            if !from.kind().is_unit() {
                continue;
            }
            let gain = to.get().effects.strength as f64 - from.get().effects.strength as f64;
            if gain > best_v {
                best = Some(m);
                best_v = gain;
            }
        }
    }
    if let Some(m) = best {
        return Some(m);
    }
    if p.workers_free > 0 && unit_workers(p) < profile.unit_cap[ctx.age as usize] {
        let mut best: Option<Move> = None;
        let mut best_v = 0.0;
        for &m in moves {
            if let Move::Build { card } = m {
                if !card.kind().is_unit() {
                    continue;
                }
                let v = card.get().effects.strength as f64;
                if v > best_v {
                    best = Some(m);
                    best_v = v;
                }
            }
        }
        if let Some(m) = best {
            return Some(m);
        }
    }
    None
}

/// Workers currently standing on military units.
fn unit_workers(p: &PlayerState) -> i32 {
    p.techs.iter().filter(|&(id, _)| id.kind().is_unit()).map(|(_, slot)| slot.workers as i32).sum()
}

/// Finish what you start -- unless the wonder is no longer worth this
/// archetype's appetite (`wonder_value(p.wonder, ..) <= 0.0`), which is how
/// `wonder_appetite`/`wonder_bonus` can make a variant walk away from a
/// half-built wonder rather than always finishing it.
fn r_wonder_step(_state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if !moves.iter().any(|m| matches!(m, Move::WonderStep { .. })) {
        return None;
    }
    if !p.wonder.is_none() && wonder_value(p.wonder, ctx, profile) <= 0.0 {
        return None;
    }
    let mut best: Option<Move> = None;
    let mut best_steps = 0u8;
    for &m in moves {
        if let Move::WonderStep { steps } = m {
            if best.is_none() || steps > best_steps {
                best = Some(m);
                best_steps = steps;
            }
        }
    }
    best
}

/// Grow whenever there is food and a job waiting, at a rate set by
/// `pop_appetite` -- but never grow into unhappiness or past the tail of the
/// game.
fn r_population(_state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if let Some(m) = moves.iter().find(|m| matches!(m, Move::PopFree)) {
        return Some(*m);
    }
    let mut combo_best: Option<Move> = None;
    let mut combo_key: (i16, &str) = (0, "");
    for &m in moves {
        if let Move::Barbarossa { card } = m {
            let key = (card.get().effects.strength, card.name());
            if combo_best.is_none() || key > combo_key {
                combo_best = Some(m);
                combo_key = key;
            }
        }
    }
    let has_pop = moves.iter().any(|m| matches!(m, Move::Pop));
    if combo_best.is_none() && !has_pop {
        return None;
    }
    let appetite = profile.pop_appetite;
    if appetite <= 0.0 {
        return None;
    }
    let idle_cap: u8 = if appetite >= 1.5 { 1 } else { 2 };
    if ctx.workers_free >= 1 && ctx.age <= 1 && appetite < 1.5 {
        return None; // a worker already idle: place it first
    }
    if ctx.workers_free >= idle_cap {
        return None;
    }
    // Growing raises the happiness requirement one band; do not step into
    // discontent.
    if economy::happy_required(p.yellow_bank.saturating_sub(1)) as i32 > ctx.s.happy {
        return None;
    }
    if ctx.late {
        return None;
    }
    if let Some(m) = combo_best {
        return Some(m);
    }
    moves.iter().find(|m| matches!(m, Move::Pop)).copied()
}

/// An idle worker produces nothing.
fn r_place_worker(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if p.workers_free == 0 {
        return None;
    }
    best_build(state, p, ctx, profile, moves, false)
}

/// Upgrading in place is efficient (one civil action, no new worker) unless
/// [`Profile::upgrade_veto`] says this archetype doesn't buy this track at
/// all (the Iron-vs-3-Bronze disagreement lives here).
fn r_upgrade(_state: &GameState, _p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::Upgrade { from, to } = m {
            let typ = to.kind();
            if typ.is_unit() {
                continue; // the military rule owns these
            }
            if table_bool_contains_type(profile.upgrade_veto, typ, ctx.nplayers) {
                continue;
            }
            let gain = prod_value(to.get().production, ctx, profile) - prod_value(from.get().production, ctx, profile);
            let cost = (to.get().resource_cost as i32 - from.get().resource_cost as i32).max(0);
            let mut v = gain - cost as f64 * 0.4;
            if typ == CardType::Farm && ctx.food_need <= 0 {
                v -= 2.0;
            }
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= 1.5 {
        best
    } else {
        None
    }
}

fn r_develop(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    best_develop(state, p, ctx, profile, moves, false)
}

/// Unweighted, matching Python: `_r_action_card` (inherited unchanged by
/// every variant) calls the module-level `_action_card_value`, not
/// `self.card_value` -- the one rule this roster's Python source never
/// routed through a profile. [`book::action_card_value`] is reused directly
/// rather than reimplemented, so this quirk is preserved by construction
/// rather than by a comment promising to keep two copies in sync.
fn r_action_card(_state: &GameState, p: &PlayerState, ctx: &Ctx, _profile: &Profile, moves: &[Move]) -> Option<Move> {
    let mut best: Option<Move> = None;
    let mut best_v = 0.0;
    for &m in moves {
        if let Move::PlayAction { card } = m {
            let v = book::action_card_value(p, ctx, card);
            if v > best_v {
                best = Some(m);
                best_v = v;
            }
        }
    }
    if best_v >= 3.0 {
        best
    } else {
        None
    }
}

fn r_take_card(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Option<Move> {
    if !moves.iter().any(|m| matches!(m, Move::Take { .. })) {
        return None;
    }
    best_take(state, p, ctx, profile, moves, false)
}

// ------------------------------------------------------------- politics

/// The attacker's strength lead over `q`, for the SAME comparison
/// `legal_moves` used to decide the move was even offered -- public
/// information (`combat::attack_strength`/`effects::state_stats`, both
/// derived from played tableaux and public pact state).
fn lead_over(state: &GameState, p: &PlayerState, q: &PlayerState) -> i32 {
    combat::attack_strength(state, p, q) - effects::state_stats(state, q).strength
}

/// Whether `p` is the (a) weakest player at the table by strength -- public
/// information, ties broken in `p`'s favour (matches Python's `<` check:
/// only a STRICTLY stronger rival disqualifies "weakest").
fn weakest(state: &GameState, p: &PlayerState) -> bool {
    let mine = effects::state_stats(state, p).strength;
    for q in state.players[..state.num_players as usize].iter() {
        if q.idx == p.idx || q.resigned {
            continue;
        }
        if effects::state_stats(state, q).strength < mine {
            return false;
        }
    }
    true
}

/// Aggressions, wars, pacts and event seeding. Mirrors `VariantBot._politics`
/// (the shared default every archetype except Military uses unchanged) with
/// [`Profile::agg_order`] folded in as an optional third sort key --
/// `MilitaryBot._politics`'s only actual deviation from the shared rule is
/// the aggression tie-break (resource/science/cube thefts cashed in before
/// culture thefts, until `ctx.age >= 3` flips the preference); its
/// weakest-player event gate is DATA-equivalent to the shared rule once
/// `seed_events_when_weakest` is false (which `military::PROFILE` sets), not
/// a second behaviour, so it needs no separate code path here.
pub(crate) fn politics(state: &GameState, p: &PlayerState, ctx: &Ctx, profile: &Profile, moves: &[Move]) -> Move {
    let lead_needed = profile.agg_lead[ctx.age as usize];
    let mut best: Option<Move> = None;
    let mut best_key: (u16, f64, &str) = (0, 0.0, "");
    for &m in moves {
        if let Move::Aggression { card, target } = m {
            if lead_over(state, p, &state.players[target as usize]) >= lead_needed {
                let culture = state.players[target as usize].culture;
                let order = match profile.agg_order {
                    Some(table) => {
                        let base = table_lookup_str(table, card.name(), 2.0);
                        if ctx.age >= 3 {
                            -base
                        } else {
                            base
                        }
                    }
                    None => 0.0,
                };
                let key = (culture, order, card.name());
                if best.is_none() || key > best_key {
                    best = Some(m);
                    best_key = key;
                }
            }
        }
    }
    if let Some(m) = best {
        return m;
    }

    if !ctx.last && ctx.age >= profile.war_from_age {
        let need = profile.war_lead;
        let mut best_w: Option<Move> = None;
        let mut best_w_key: (u16, &str) = (0, "");
        for &m in moves {
            if let Move::War { card, target } = m {
                if lead_over(state, p, &state.players[target as usize]) >= need {
                    let key = (state.players[target as usize].culture, card.name());
                    if best_w.is_none() || key > best_w_key {
                        best_w = Some(m);
                        best_w_key = key;
                    }
                }
            }
        }
        if let Some(m) = best_w {
            return m;
        }
    }

    // A pact is worth having when it is not a gift to a runaway leader.
    let mut best_pact: Option<Move> = None;
    let mut best_pact_key: (&str, u8, u8) = ("", 0, 3);
    for &m in moves {
        if let Move::OfferPact { card, target, side } = m {
            if state.players[target as usize].culture <= p.culture {
                let side_rank = match side {
                    PactSide::Unspecified => 0u8,
                    PactSide::A => 1,
                    PactSide::B => 2,
                };
                let key = (card.name(), target, side_rank);
                if best_pact.is_none() || key < best_pact_key {
                    best_pact = Some(m);
                    best_pact_key = key;
                }
            }
        }
    }
    if let Some(m) = best_pact {
        return m;
    }

    if moves.iter().any(|m| matches!(m, Move::PrepareEvent { .. })) {
        if !profile.seed_events_when_weakest && weakest(state, p) {
            return Move::PolPass;
        }
        let mut best_ev: Option<Move> = None;
        let mut best_ev_name = "";
        for &m in moves {
            if let Move::PrepareEvent { card } = m {
                let name = card.name();
                if best_ev.is_none() || name < best_ev_name {
                    best_ev = Some(m);
                    best_ev_name = name;
                }
            }
        }
        if let Some(m) = best_ev {
            return m;
        }
    }
    Move::PolPass
}

// --------------------------------------------------------------- the bot

/// The six archetypes. `Archetype::ALL` and [`Archetype::name`] are the
/// registry [`super::greedy::BotKind`] reads to make each one selectable by
/// name -- see `bots::greedy`'s own `BotKind::Culture`/etc. variants.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Archetype {
    Culture,
    Military,
    Science,
    Wonder,
    Infra,
    Tempo,
}

impl Archetype {
    pub const ALL: &'static [Archetype] =
        &[Archetype::Culture, Archetype::Military, Archetype::Science, Archetype::Wonder, Archetype::Infra, Archetype::Tempo];

    pub const fn name(self) -> &'static str {
        match self {
            Archetype::Culture => "culture",
            Archetype::Military => "military",
            Archetype::Science => "science",
            Archetype::Wonder => "wonder",
            Archetype::Infra => "infra",
            Archetype::Tempo => "tempo",
        }
    }

    fn profile(self) -> &'static Profile {
        match self {
            Archetype::Culture => &culture::PROFILE,
            Archetype::Military => &military::PROFILE,
            Archetype::Science => &science::PROFILE,
            Archetype::Wonder => &wonder::PROFILE,
            Archetype::Infra => &infrastructure::PROFILE,
            Archetype::Tempo => &tempo::PROFILE,
        }
    }

    fn rules(self) -> &'static [RuleId] {
        match self {
            Archetype::Culture => culture::RULES,
            Archetype::Military => military::RULES,
            Archetype::Science => science::RULES,
            Archetype::Wonder => wonder::RULES,
            Archetype::Infra => infrastructure::RULES,
            Archetype::Tempo => tempo::RULES,
        }
    }
}

/// One archetype from the roster. Mirrors `VariantBot` (the shared base) +
/// whichever `*Bot` subclass `archetype` names -- a single struct rather
/// than one Rust type per archetype, since (see this module's top doc
/// comment) every archetype is fully described by a `Profile`/`RULES` data
/// pair, not by distinct code.
#[derive(Clone, Copy, Debug)]
pub struct VariantBot {
    pub archetype: Archetype,
}

impl VariantBot {
    pub fn new(archetype: Archetype) -> VariantBot {
        VariantBot { archetype }
    }

    /// Entry point, matching [`BookBot::choose`]'s contract: `moves` should
    /// be exactly what [`crate::legal::legal_moves`] returns for `state`.
    ///
    /// # Panics
    /// If `moves` is empty (see [`BookBot::choose`]'s identical panic note).
    pub fn choose(&self, state: &GameState, moves: &[Move]) -> Move {
        let filtered = super::filter_resign(moves, false);
        let moves = filtered.as_slice();
        if moves.len() == 1 {
            return moves[0];
        }
        if !state.pending.is_empty() {
            // No Python variant overrides pending-decision handling -- see
            // this module's top doc comment. `version: 2` because every
            // variant is built on BookBot's v2 (empirical tournament tier
            // list) rules, matching `VariantBot.__init__`'s
            // `super().__init__(..., version=2, ...)`.
            let book_bot = BookBot { version: 2, tunables: V2Tunables::default() };
            return book_bot.pending_pick(state, moves);
        }
        let p_idx = state.actor().idx;
        let p = &state.players[p_idx as usize];
        let ctx = Ctx::new(state, p_idx, 2, V2Tunables::default());
        let profile = self.archetype.profile();
        if state.phase == Phase::Politics {
            politics(state, p, &ctx, profile, moves)
        } else {
            action_phase(state, p, &ctx, profile, self.archetype.rules(), moves)
        }
    }

    pub fn pick(&self, state: &GameState) -> Move {
        let moves = crate::legal::legal_moves(state);
        self.choose(state, moves.as_slice())
    }
}

// ================================================================== tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game;

    /// Every archetype must produce a legal game from turn 1 to completion
    /// without panicking -- the cheapest possible "the port is not
    /// nonsense" check, run for all six before any identity-specific test.
    #[test]
    fn every_archetype_plays_a_full_game_to_completion_without_panicking() {
        for &archetype in Archetype::ALL {
            let bot = VariantBot::new(archetype);
            let mut state = game::new_game(2, 12345);
            let mut turns = 0;
            loop {
                if state.game_over {
                    break;
                }
                let mv = bot.pick(&state);
                crate::apply::apply(&mut state, mv);
                turns += 1;
                assert!(turns < 5000, "{} did not finish within 5000 decisions", archetype.name());
            }
        }
    }

    /// [`VariantBot::choose`] must never mutate the state it was handed --
    /// matches the same test in `book.rs`/`greedy.rs`/`quiescent.rs`.
    #[test]
    fn choose_never_mutates_the_real_state() {
        let state = game::new_game(2, 7);
        let before = state.clone();
        let moves = crate::legal::legal_moves(&state);
        let bot = VariantBot::new(Archetype::Culture);
        let _ = bot.choose(&state, moves.as_slice());
        assert_eq!(state.round, before.round);
        assert_eq!(state.players[0].resources, before.players[0].resources);
        assert_eq!(state.card_row, before.card_row);
    }

    /// [`VariantBot::choose`] always returns one of the offered moves (never
    /// invents one) -- the same contract every other bot in this crate
    /// tests for itself.
    #[test]
    fn choose_always_returns_one_of_the_offered_moves() {
        let state = game::new_game(3, 99);
        let moves = crate::legal::legal_moves(&state);
        for &archetype in Archetype::ALL {
            let bot = VariantBot::new(archetype);
            let mv = bot.choose(&state, moves.as_slice());
            assert!(moves.as_slice().contains(&mv), "{} chose a move not in the offered list", archetype.name());
        }
    }

    /// `rank_of`-style tables key by [`CardId::name`], which is only checked
    /// against real cards by [`book`]'s own
    /// `every_rank_table_name_resolves_to_a_real_card` test for ITS tables.
    /// This is the same check for every name-keyed [`Profile`] table in the
    /// roster: a typo here silently defaults to "no bonus" instead of
    /// failing loudly, so walk every entry through `CardId::by_name`.
    #[test]
    fn every_profile_table_name_resolves_to_a_real_card() {
        fn check_f64(label: &str, table: &[(&str, Pc<f64>)]) {
            for &(name, _) in table {
                assert!(CardId::by_name(name).is_some(), "{label}: no card named {name:?}");
            }
        }
        fn check_bool(label: &str, table: &[(&str, Pc<bool>)]) {
            for &(name, _) in table {
                assert!(CardId::by_name(name).is_some(), "{label}: no card named {name:?}");
            }
        }
        for &archetype in Archetype::ALL {
            let p = archetype.profile();
            check_f64(archetype.name(), p.tech_bonus);
            check_bool(archetype.name(), p.tech_veto);
            check_f64(archetype.name(), p.card_bonus);
            check_f64(archetype.name(), p.leader_bonus);
            check_f64(archetype.name(), p.wonder_bonus);
            for &name in p.must_buy_3ca {
                assert!(CardId::by_name(name).is_some(), "{}: no card named {name:?}", archetype.name());
            }
            if let Some(table) = p.agg_order {
                for &(name, _) in table {
                    assert!(CardId::by_name(name).is_some(), "{}: no card named {name:?}", archetype.name());
                }
            }
        }
    }
}
