//! Event-gain application (§5.3/§5.4.6) and §12.5.2 final scoring. Ports
//! `engine/events.py`'s "gain blocks" section (that file's own header
//! comment, lines 41-125: `apply_gains` and its one directly-needed helper,
//! `_food_or_resources`, this module's [`food_or_resources`]) and, added
//! 2026-08-05, its final-scoring section (`pending_final_events`,
//! `scoring_culture`, `final_event_awards`, `evaluate_final_events` --
//! [`final_event_awards`]/[`evaluate_final_events`] below).
//!
//! ## Scope: what this module is, and is not
//!
//! `apply_gains` is the shared interpreter Python uses BOTH to award an
//! event's effect block to a player (`_apply_player_block`, from
//! `resolve_event`) AND to pay an aggression's success gains to the attacker
//! (`finish_aggression`). Only the second caller is wired up by this pass --
//! `combat.rs::finish_aggression`'s SUCCESS branch, which used to
//! `unimplemented!` naming this module. Event RESOLUTION DURING PLAY
//! (`resolve_event`, `_apply_player_block`, `_conditional_target`,
//! `_apply_extras`) is a separate, larger job and is deliberately not
//! touched here -- see "keys this module does not implement" below for
//! exactly where that leaves `apply_gains` short of Python's version, and
//! why that shortfall is inert today.
//!
//! §12.5.2 final scoring is a DIFFERENT, self-contained slice of
//! `engine/events.py` that this pass DOES port in full: the fifteen
//! "Impact of ..." Age III event cards still sitting in `current_events`/
//! `future_events` at game end score directly off their own printed data
//! (`cards::FinalScoringBlock`, built by `gen_cards.py` from each card's
//! `effects.allPlayers`), with no dependency on `resolve_event` or anything
//! else still unported.
//!
//! In Python's actual rules, one of these 15 cards CAN also score early: if
//! revealed during play, `resolve_event` moves it to `past_events` and
//! `_apply_player_block` calls `scoring_culture` (and the same
//! `rankingCulture` logic [`final_event_awards`] below reimplements) right
//! then, banking the culture in `p.culture` immediately rather than at game
//! end -- which is exactly why [`pending_final_events`] excludes
//! `past_events`, the same exclusion Python's own function documents.
//! `resolve_event`/`_apply_player_block` are out of THIS pass's scope and,
//! separately, are not reachable at all yet in this port (`Move::
//! PrepareEvent` is `unimplemented!()` in `apply.rs`, and nothing seeds an
//! Age III event into `current_events`/`future_events` either -- see
//! `game.rs`'s KNOWN GAPS) -- so today this module is the ONLY path any Age
//! III event can score through, but the `past_events` boundary itself is a
//! real rule this port will still need the day event-revealing lands, not a
//! today-only shortcut invented here.
//!
//! `_draw_military` (`engine/events.py:117-124`) is NOT ported: nothing in
//! Python's `apply_gains` can reach it except through a `drawMilitaryCards`
//! key, and -- see below -- no card this port can construct a block from
//! ever carries one. Porting a function apply_gains cannot currently call
//! would be exactly the untested-dead-code shape this project's structural
//! guarantees exist to prevent (a compile-time-exhaustive `match` is only a
//! guarantee if every arm is reachable).
//!
//! ## What `apply_gains` operates on
//!
//! Python's `apply_gains(state, p, block, rng, sign)` takes an arbitrary
//! dict -- sometimes a whole card's `effects`, sometimes a nested sub-dict an
//! event prints under `allPlayers`/`weakestPlayer`/etc (§5.3). Every key
//! inside one of those NESTED sub-dicts is opaque to this port today:
//! `gen_cards.py`'s `DEFERRED_DICT_EFFECT_KEYS` still collapses `allPlayers`
//! and its seven siblings to a payload-less `Special` ("event targeting --
//! events.rs not ported"), so there is no Rust value yet that could stand in
//! for one of those nested dicts.
//!
//! The one shape IS ported: a CARD's own top-level `effects` dict, which
//! `gen_cards.py` already decodes unconditionally for every card (via
//! `CardEffects`'s recurring fields, plus whichever one-off `Special`
//! variants that card's other keys produced) regardless of whether
//! resolution is wired up. That is exactly what `combat.rs::
//! finish_aggression` passes today (Python: `apply_gains(state, attacker,
//! eff, rng)` where `eff = db.get(name).get("effects")`, `name` being the
//! AGGRESSION card itself). So [`apply_gains`] here takes a `CardId` and
//! reads `card.get().effects` / `card.get().special` directly, rather than a
//! generic block value -- there being no other value it is ever asked to
//! operate on by this port's one caller.
//!
//! Reusing `CardEffects` (rather than a dedicated struct, the way
//! `PactBlock` exists precisely to avoid overloading `CardEffects`) is safe
//! here for a structural reason, not a coincidental one: `effects::compute`
//! only ever reads a `CardEffects` off a `CardId` sitting in one of a
//! player's SLOTS (`p.techs`/`wonder`/`tactic`/`government`/`leader`).
//! Event/aggression/war cards are never placed in any of those slots -- they
//! resolve and are discarded -- so `compute` structurally cannot ever read
//! the very fields [`apply_gains`] is about to interpret as one-shot gains.
//! (Contrast a hypothetical territory card: its `CardEffects.food`/
//! `resources` ARE read recurringly, once colonies enter play, via
//! `permanentEffects` -- which is exactly why `apply_gains` is never called
//! with a territory `CardId` by anything in this port; colonization is its
//! own unported area.)
//!
//! ## Keys this module does not implement, and why
//!
//! FLAGGED, not routed around (this project's standing rule: reproduce a
//! real gap faithfully and say so, rather than silently drop it). Eight of
//! Python's `apply_gains` key branches have no reachable path through this
//! port's one call site, verified against a full top-level `effects`-key
//! census over all 236 base-2015 cards (2026-08-05):
//!
//!   * `loseScience`, `loseCulture`, `population`/`gainPopulation`,
//!     `increasePopulation`, a BARE (top-level) `yellowTokens`, a BARE
//!     `loseAllStoredFood`, a BARE `foodAndOrResources` -- printed by ZERO
//!     cards anywhere in the base game's data, at ANY nesting depth.
//!     `gen_cards.py` only emits a `Special` variant for a key it has
//!     actually seen (`card_table.rs`'s own doc comment: "one variant per
//!     distinct one-off effect key"), so there is no variant for any of
//!     these six and no card's `special` slice could ever carry one. A match
//!     arm against a variant that cannot exist would be dead code nothing
//!     exercises -- the opposite of "a card whose rule the engine cannot
//!     interpret is a compile error". If a future data revision (or the
//!     expansion, out of scope by standing decision) ever prints one,
//!     `gen_cards.py`'s exhaustive key census fails the build and names it,
//!     at which point it gets a real field/variant and a real arm here.
//!   * `drawMilitaryCards` -- printed exactly once (`Development of
//!     Politics`), but NESTED under `allPlayers`, the opaque-dict case
//!     above; the key exists, but no `CardId`-shaped value this module can
//!     read carries it.
//!   * `decreasePopulation`/`losePopulation` -- IS printed top-level
//!     (Barbarians: `Special::DecreasePopulation`), so it IS implemented
//!     below, but note it is never actually reached by Python's own
//!     `resolve_event` either: Barbarians has no `allPlayers` key, so its
//!     `decreasePopulation` is read directly by `_conditional_target`, not
//!     through `apply_gains` -- included here anyway because it is a real,
//!     data-backed branch, exercised by this module's own unit test rather
//!     than by any live caller today.
//!
//! `food`/`resources` (bare, i.e. not `gainFood`/`gainResources`) ARE
//! backed by a field (`CardEffects.food`/`resources`) and so are handled
//! below despite being unreachable by any base-game aggression card either
//! -- unlike the six above, a card COULD print them (territories do, via a
//! different JSON path merged into the same field by `gen_cards.py`; see
//! `cards.rs`'s doc comment on `CardEffects.food`), so there is a real
//! variant to dispatch on, just no aggression/event card that currently
//! does.

use crate::cards::{Age, CardId, CardType, FinalScoringStat, Special};
use crate::economy;
use crate::effects;
use crate::game;
use crate::interact;
use crate::state::{GameState, PlayerState, QueueItem};

/// §5.3/§5.4.6: apply one card's own top-level gain effects to player `idx`.
/// Mirrors `engine/events.py::apply_gains` -- see this module's top doc
/// comment for exactly which of Python's key branches are implemented, why
/// the rest are not, and why `card: CardId` stands in for Python's
/// `block: dict`.
///
/// `sign = -1` inverts every gain into a loss (Python's own docstring:
/// "`sign=-1` inverts (lose blocks)"). `combat::finish_aggression` always
/// calls this with `sign = 1` (an aggression's own effects are gains to the
/// ATTACKER, never losses) -- the parameter exists anyway because it is the
/// one thing separating this from a second, near-identical copy of the
/// function the moment a lose-block caller exists, exactly as in Python.
pub fn apply_gains(state: &mut GameState, idx: u8, card: CardId, sign: i32) {
    let eff = card.get().effects;

    // science / gainScience (events.py:47-49) -- both keys apply
    // identically in Python, so both fields are walked the same way. Each
    // is its own statement (not summed first) so a card that somehow
    // printed both would clamp at zero exactly where Python's per-key dict
    // loop would, rather than only after combining them.
    for delta in [eff.science, eff.gain_science] {
        add_clamped(&mut state.players[idx as usize].science, sign * delta as i32);
    }
    // culture / gainCulture (events.py:53-55).
    for delta in [eff.culture, eff.gain_culture] {
        add_clamped(&mut state.players[idx as usize].culture, sign * delta as i32);
    }
    // food / gainFood (events.py:59-64). `produceFood` is the third key
    // spelling Python accepts here, but it is only ever printed as a
    // BOOLEAN flag in the base data (`_num` rejects bools -- see
    // `_apply_extras`, out of this port's scope, which is what actually
    // reads that flag), so it never contributes a magnitude through this
    // path regardless.
    for delta in [eff.food, eff.gain_food] {
        apply_food_delta(state, idx, delta, sign);
    }
    // resources / gainResources (events.py:65-70). Same `produceResources`
    // caveat as food above.
    for delta in [eff.resources, eff.gain_resources] {
        apply_resources_delta(state, idx, delta, sign);
    }
    // blueTokens (events.py:91-93).
    if eff.blue_tokens != 0 {
        let p = &mut state.players[idx as usize];
        p.blue_total = (p.blue_total as i32 + sign * eff.blue_tokens as i32).max(0) as u8;
    }
    // strength (events.py:94-96) -- a ONE-SHOT grant via `strength_extra`,
    // not the recurring per-turn `CardEffects.strength` a card sitting in a
    // player's tableau contributes through `effects::compute`. See this
    // module's top doc comment for why the two meanings cannot collide.
    if eff.strength != 0 {
        state.players[idx as usize].strength_extra += (sign as i16) * eff.strength;
    }
    // happiness / happy (events.py:97-99) -- same one-shot-via-`_extra`
    // reasoning as strength above.
    if eff.happy != 0 {
        state.players[idx as usize].happy_extra += (sign as i16) * eff.happy;
    }
    // decreasePopulation / losePopulation (events.py:82-87) -- §6.5/FAQ
    // p.15: the OWNER chooses which worker to lose, so this only enqueues
    // the decision. Only the `decreasePopulation` spelling is ever printed
    // top-level in the base data (Barbarians); `Special::DecreasePopulation`
    // is the int-shape variant `gen_cards.py` already emits for it.
    for &sp in card.get().special {
        if let Special::DecreasePopulation(n) = sp {
            if n != 0 {
                interact::enqueue(state, QueueItem::LosePop { player: idx, n: n as u8 });
            }
        }
    }
}

/// `p.X = max(0, p.X + delta)` (events.py's repeated `p.science = max(0,
/// p.science + sign * v)` idiom) -- a no-op when `delta` is zero, matching
/// Python's own `if v:` guard on every branch that uses it.
fn add_clamped(field: &mut u16, delta: i32) {
    if delta != 0 {
        *field = (*field as i32 + delta).max(0) as u16;
    }
}

/// One `food`/`gainFood` key application (events.py:59-64): the blue-token-
/// limited [`economy::gain_food`] when gaining, plain floored subtraction
/// when losing -- `sign` chooses which, `p.food` itself is never allowed
/// negative either way.
fn apply_food_delta(state: &mut GameState, idx: u8, delta: i16, sign: i32) {
    if delta == 0 {
        return;
    }
    if sign > 0 {
        economy::gain_food(&mut state.players[idx as usize], delta as u16);
    } else {
        let p = &mut state.players[idx as usize];
        p.food = p.food.saturating_sub(delta as u16);
    }
}

/// The `resources`/`gainResources` twin of [`apply_food_delta`]
/// (events.py:65-70).
fn apply_resources_delta(state: &mut GameState, idx: u8, delta: i16, sign: i32) {
    if delta == 0 {
        return;
    }
    if sign > 0 {
        economy::gain_resources(&mut state.players[idx as usize], delta as u16);
    } else {
        let p = &mut state.players[idx as usize];
        p.resources = p.resources.saturating_sub(delta as u16);
    }
}

/// §5.4.6/§11.5: move `amount` between food and resources, preferring
/// resources both ways -- gaining tops up resources first and spills the
/// remainder into food (blue-token limited, via [`economy::gain_resources`]/
/// [`economy::gain_food`]); losing drains resources first and only then
/// food (unlimited, floored at zero). Mirrors `engine/events.py::
/// _food_or_resources`.
///
/// This is [`apply_gains`]'s OWN helper for its (unreachable, see this
/// module's top doc comment) bare `foodAndOrResources` key -- but it is very
/// much live for real: `combat::finish_aggression`'s `takeFromOpponent.
/// foodAndOrResources` theft (`events.py:656-659`, Aggression: Plunder) calls
/// this exact private function too, so it is `pub(crate)` rather than
/// private, and there is one copy, not two drifting in and out of step.
pub(crate) fn food_or_resources(p: &mut PlayerState, amount: i32, sign: i32) {
    let amount = amount.max(0) as u16;
    if sign > 0 {
        let got = economy::gain_resources(p, amount);
        economy::gain_food(p, amount - got);
    } else {
        let take = p.resources.min(amount);
        p.resources -= take;
        p.food = p.food.saturating_sub(amount - take);
    }
}

// ==================================================== §12.5.2 final scoring

/// The Age III scoring events still owed a payout, in scoring order.
/// Mirrors `engine/events.py::pending_final_events`, including its exclusion
/// of `past_events` -- see this module's top doc comment for why an event
/// there already paid out through `_apply_player_block`.
///
/// Python returns `[(name, block), ...]`; this returns just the `CardId`s,
/// since `block` here is `card.get().special`'s own `Special::FinalScoring`
/// payload rather than a separate dict -- [`final_scoring_block`] reads it
/// straight off the card, so there is nothing a second return value would
/// carry that the caller cannot already get from the `CardId` alone.
fn pending_final_events(state: &GameState) -> Vec<CardId> {
    state
        .current_events
        .as_slice()
        .iter()
        .chain(state.future_events.as_slice().iter())
        .copied()
        .filter(|c| !c.is_none() && final_scoring_block(*c).is_some())
        .collect()
}

/// The `Special::FinalScoring` payload off one card, if it has one. `None`
/// for every card except the 15 base-game `scoringEvent` events (see
/// `cards::FinalScoringBlock`'s doc comment).
fn final_scoring_block(card: CardId) -> Option<&'static crate::cards::FinalScoringBlock> {
    card.get().special.iter().find_map(|sp| match sp {
        Special::FinalScoring(block) => Some(block),
        _ => None,
    })
}

/// Live players in clockwise turn order starting at `state.start_player`.
/// Mirrors `engine/events.py::_order_from(state, first_idx)`, but narrowed
/// to the one `first_idx` [`final_event_awards`] ever calls it with --
/// Python's other three call sites (`resolve_event`, `_conditional_target`,
/// `interact.start_auction`) start from the revealer/current player instead,
/// and are out of this pass's scope.
fn order_from_start(state: &GameState) -> Vec<u8> {
    let n = state.num_players;
    (0..n)
        .map(|i| (state.start_player + i) % n)
        .filter(|&idx| !state.players[idx as usize].resigned)
        .collect()
}

/// The one statistic value `final_event_awards`'s `rankingCulture` ranking
/// ever needs. Mirrors `engine/events.py::_stat_value`, narrowed to the five
/// `FinalScoringStat` targets `_STAT_ALIASES` maps final-scoring's
/// `statistic` key onto -- Python's fuller version also answers
/// `happy`/`discontent`/`culture` for `resolve_event`'s targeting callers,
/// out of scope here.
fn final_scoring_stat_value(state: &GameState, p: &PlayerState, stat: FinalScoringStat) -> i32 {
    let s = effects::state_stats(state, p);
    match stat {
        FinalScoringStat::Strength => s.strength,
        FinalScoringStat::Science => s.science,
        FinalScoringStat::CultureRate => s.culture,
        FinalScoringStat::Food => s.food,
        FinalScoringStat::Resources => s.resources,
    }
}

/// `order`'s players, best-`stat`-first, ties broken by position in `order`
/// (§5.3 tie-break: whoever is closer to the start player in turn order).
/// Mirrors `engine/events.py::_rank(state, order, stat, best_first=True)` --
/// the only `best_first` value `final_event_awards` ever passes -- via a
/// stable sort on `order` itself: `order` is already in tie-break order, so
/// a stable sort preserves ties in exactly the position Python's explicit
/// `idx[q.idx]` secondary key does, with no second key needed.
fn rank_by_final_scoring_stat(state: &GameState, order: &[u8], stat: FinalScoringStat) -> Vec<u8> {
    let mut ranked = order.to_vec();
    ranked.sort_by_key(|&idx| -final_scoring_stat_value(state, &state.players[idx as usize], stat));
    ranked
}

/// Culture awarded by ONE "Impact of ..." Age III scoring event to ONE
/// player. Ports `engine/events.py::scoring_culture`'s per-key dispatch --
/// `block`'s fields are [`crate::cards::FinalScoringBlock`]'s zero-default
/// fields rather than a dict entry per printed key, so every branch below
/// runs unconditionally and is a no-op on a card that did not print that
/// key, the same "0 = not printed" convention `TakeFromOpponentBlock`'s/
/// `PactBlock`'s magnitude fields already use.
///
/// Python's `scoring_culture(state, p, block, order)` takes a fourth `order`
/// parameter its own body never reads anywhere (checked 2026-08-05) -- a
/// dead parameter, not ported.
fn scoring_culture(
    state: &GameState,
    p: &PlayerState,
    block: &crate::cards::FinalScoringBlock,
) -> i32 {
    let s = effects::state_stats(state, p);
    let mut total: i32 = 0;

    // culturePerResourceProducedByMines -- "Impact of Industry".
    total += block.culture_per_resource_produced_by_mines as i32 * effects::mine_resources(p);

    // culturePerFoodProducedByFarms (+ bonusIfProductionExceedsConsumption)
    // -- "Impact of Agriculture". The two keys only ever co-occur on this
    // one base-game card (see `FinalScoringBlock`'s doc comment), so gating
    // the bonus on its own nonzero value alone reproduces Python's
    // per-key-branch reading exactly for every card that exists today.
    total += block.culture_per_food_produced_by_farms as i32 * effects::farm_food(p);
    if block.bonus_if_production_exceeds_consumption != 0
        && s.food > economy::consumption(p.yellow_bank) as i32
    {
        total += block.bonus_if_production_exceeds_consumption as i32;
    }

    // culturePerLevelOfMilitaryUnitsAndArenas -- "Impact of Competition".
    for (id, slot) in p.techs.iter() {
        if id.kind().is_unit() || id.kind() == CardType::Arena {
            total += block.culture_per_level_of_military_units_and_arenas as i32
                * id.level() as i32
                * slot.workers as i32;
        }
    }

    // culturePerLevelOfSpecialTechsAndGovernment -- "Impact of Progress".
    let mut tech_and_gov_levels = p.government.level() as i32;
    for (id, _) in p.techs.of_type(CardType::SpecialTech) {
        tech_and_gov_levels += id.level() as i32;
    }
    total += block.culture_per_level_of_special_techs_and_government as i32 * tech_and_gov_levels;

    // culturePerCompletedWonderByAge -- "Impact of Wonders": a per-age
    // table, indexed by `Age as u8` exactly like `Special::BuildDiscount`.
    for &w in p.completed_wonders.as_slice() {
        total += block.culture_per_completed_wonder_by_age[w.get().age as usize] as i32;
    }

    // culturePerContentWorkerAbove10 -- "Impact of Population". A yellow
    // token in the worker pool is a worker too (events.py's own comment: "a
    // discontent worker is physically an unused worker moved onto the
    // happiness track"), so this counts on-card workers PLUS
    // `workers_free`, minus discontent.
    let workers: i32 =
        p.techs.iter().map(|(_, slot)| slot.workers as i32).sum::<i32>() + p.workers_free as i32;
    let content = (workers - economy::discontent(state, p)).max(0);
    total += block.culture_per_content_worker_above_10 as i32 * (content - 10).max(0);

    // culturePerColony -- "Impact of Colonies".
    total += block.culture_per_colony as i32 * p.colonies.len() as i32;

    // culturePerCivilAction / culturePerMilitaryAction -- "Impact of
    // Government". Scores the player's current ACTION ALLOWANCE
    // (`state_stats`'s `civil_actions`/`military_actions`), not actions
    // actually spent over the course of the game -- matching Python's own
    // `s.civil_actions`/`s.military_actions` read exactly.
    total += block.culture_per_civil_action as i32 * s.civil_actions;
    total += block.culture_per_military_action as i32 * s.military_actions;

    // culturePerLevelOfUrbanBuildings -- "Impact of Architecture".
    for (id, slot) in p.techs.iter() {
        if id.kind().is_urban() {
            total += block.culture_per_level_of_urban_buildings as i32
                * id.level() as i32
                * slot.workers as i32;
        }
    }

    // culturePerHappyFace (+ maxCultureFromHappyFaces cap) -- "Impact of
    // Happiness". `max_culture_from_happy_faces == 0` means "no cap" -- see
    // `FinalScoringBlock`'s doc comment; no base-game card prints an actual
    // cap of 0.
    let mut happy_gain = block.culture_per_happy_face as i32 * s.happy;
    if block.max_culture_from_happy_faces != 0 {
        happy_gain = happy_gain.min(block.max_culture_from_happy_faces as i32);
    }
    total += happy_gain;

    // culturePerDiscontentWorker -- also "Impact of Happiness" (negative on
    // that card: -2 per discontent worker).
    total += block.culture_per_discontent_worker as i32 * economy::discontent(state, p);

    // culturePerAgeIIITechnology -- "Impact of Technology".
    let mut n_iii = p.techs.iter().filter(|(id, _)| id.get().age == Age::III).count() as i32;
    if p.government.get().age == Age::III {
        n_iii += 1;
    }
    total += block.culture_per_age_iii_technology as i32 * n_iii;

    // cultureTimesLowestProduction -- "Impact of Balance".
    total +=
        block.culture_times_lowest_production as i32 * s.food.min(s.resources).min(s.science).min(s.culture);

    // culturePerDistinctTypeOfUnitUrbanBuildingAndSpecialTech -- "Impact of
    // Variety". Python builds this by unioning distinct CARD TYPES (staffed
    // urban/unit buildings) with distinct CARD NAMES (special techs) into
    // ONE set -- a special tech is always unique by construction, so that
    // union is exactly "count of distinct types present" + "count of
    // special techs held"; counted that way directly here (a bitmask over
    // `CardType`, DESIGN.md rule 3) instead of replicating the string-set
    // trick.
    let mut urban_or_unit_kinds: u32 = 0;
    for (id, slot) in p.techs.iter() {
        let kind = id.kind();
        if slot.workers > 0 && (kind.is_urban() || kind.is_unit()) {
            urban_or_unit_kinds |= 1 << (kind as u32);
        }
    }
    let distinct = urban_or_unit_kinds.count_ones() as i32
        + p.techs.of_type(CardType::SpecialTech).count() as i32;
    total += block.culture_per_distinct_type_of_unit_urban_building_and_special_tech as i32
        * distinct;

    total
}

/// THE Age III final-scoring calculation (§12.5.2). Everything else calls
/// this. Mirrors `engine/events.py::final_event_awards`.
///
/// Returns one entry per pending scoring event, holding the individual
/// culture awards **in the order the engine applies them** -- for each
/// player in turn order starting at `state.start_player`, the
/// [`scoring_culture`] award and then, if the event carries a ranking table,
/// the `rankingCulture` award. The step list is deliberately not pre-summed:
/// [`evaluate_final_events`] clamps a player's running culture at zero after
/// EACH award, so a player near zero with a net-negative scoring board gets
/// a different total from the pooled sum.
///
/// Python recomputes its `_rank` call fresh on every iteration of the
/// per-player loop (`events.py:583`), even though nothing in the loop body
/// changes it between iterations; this hoists it once per event instead --
/// same result, no wasted work, not a behaviour change.
pub fn final_event_awards(state: &GameState) -> Vec<(CardId, Vec<(u8, i32)>)> {
    let order = order_from_start(state);
    let live = game::live_count(state);
    let mut out = Vec::new();
    for card in pending_final_events(state) {
        let block = final_scoring_block(card).expect(
            "pending_final_events only returns cards final_scoring_block resolves to Some",
        );
        let ranked = if block.has_ranking {
            Some(rank_by_final_scoring_stat(state, &order, block.ranking_stat))
        } else {
            None
        };
        let table: &[i16] = match live {
            2 => &block.ranking_2p,
            3 => &block.ranking_3p,
            _ => &block.ranking_4p,
        };

        let mut steps = Vec::new();
        for &idx in &order {
            let p = &state.players[idx as usize];
            steps.push((idx, scoring_culture(state, p, block)));
            if let Some(ranked) = &ranked {
                if let Some(pos) = ranked.iter().position(|&r| r == idx) {
                    if pos < table.len() {
                        steps.push((idx, table[pos] as i32));
                    }
                }
            }
        }
        out.push((card, steps));
    }
    out
}

/// Applies [`final_event_awards`] -- it does not recompute anything. Mirrors
/// `engine/events.py::evaluate_final_events`; the starting player counts as
/// the current player (`order_from_start`).
///
/// Python's `if state.has_military:` guard is not reproduced: `has_military`
/// is a card-database-completeness flag, always true for this engine's
/// always-complete 236-card table (same reasoning `legal.rs::politics_moves`
/// already documents for the same guard), so there is no `state.has_military`
/// field to read. Python also `state.emit`s a log line per event; there is
/// no journal/emit sink in this port and nothing reads the string (same
/// treatment `economy.rs::end_of_turn` already gives every other dropped
/// `emit` call).
pub fn evaluate_final_events(state: &mut GameState) {
    for (_card, steps) in final_event_awards(state) {
        for (idx, amount) in steps {
            if amount != 0 {
                let p = &mut state.players[idx as usize];
                p.culture = (p.culture as i32 + amount).max(0) as u16;
            }
        }
    }
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::CardId;
    use crate::state::{
        CardList, GameState, PactList, Phase, PlayerState, Tableau, MAX_PLAYERS, ROW_SIZE,
    };

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    fn blank_player(idx: u8, government: CardId) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government,
            leader: CardId::NONE,
            used_leader_ability: false,
            wonder: CardId::NONE,
            wonder_steps: 0,
            completed_wonders: CardList::new(),
            destroyed_wonders: 0,
            homer_wonder: CardId::NONE,
            tactic: CardId::NONE,
            tactic_exclusive: false,
            colonies: CardList::new(),
            flipped_wonders: CardList::new(),
            taken_leader_ages: 0,
            war_declared_by_me: CardId::NONE,
            war_target: 0,
            wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
            pacts: PactList::new(),
            hand_civil: CardList::new(),
            hand_military: CardList::new(),
            hidden_civil: 0,
            hidden_military: 0,
            yellow_bank: 0,
            yellow_granted: 0,
            workers_free: 0,
            blue_total: 0,
            food: 0,
            resources: 0,
            science: 0,
            culture: 0,
            culture_rate_extra: 0,
            science_rate_extra: 0,
            strength_extra: 0,
            happy_extra: 0,
            civil_actions: 0,
            military_actions: 0,
            politics_done: false,
            tactic_action_used: false,
            taken_this_turn: CardList::new(),
            ca_spent_taking: 0,
            hammurabi_used: false,
            churchill_used: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: crate::state::OneTimeDiscount::default(),
            resigned: false,
        }
    }

    fn one_player_state(p0: PlayerState) -> GameState {
        let filler = || blank_player(1, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p0;
        GameState {
            num_players: 2,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 2,
            start_player: 0,
            age_civil: crate::cards::Age::A,
            age_military: crate::cards::Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: crate::cards::Age::A,
            scoring_events: CardList::new(),
            available_tactics: CardList::new(),
            civil_discard: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            civil_removed: [CardList::new(), CardList::new(), CardList::new(), CardList::new(), CardList::new()],
            discarded_military: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            last_round: false,
            final_round_end: None,
            game_over: false,
            phase: Phase::Actions,
            forced_winner: None,
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
        }
    }

    // ------------------------------------------------------------ apply_gains

    #[test]
    fn apply_gains_enslave_grants_food_and_resources_to_the_attacker() {
        // The one base-game aggression card whose top-level effects apply_gains
        // actually does anything with (see this module's top doc comment):
        // "Aggression: Enslave" prints `{gainFood: 2, gainResources: 2,
        // opponentDecreasesPopulation: 1}` -- the last key is combat.rs's job,
        // not apply_gains's.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.blue_total = 10; // enough blue tokens for both gains to land.
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Enslave"), 1);
        assert_eq!(state.players[0].food, 2);
        assert_eq!(state.players[0].resources, 2);
    }

    #[test]
    fn apply_gains_is_a_no_op_for_a_takefromopponent_only_card() {
        // "Aggression: Spy" prints only `takeFromOpponent`, which apply_gains
        // does not read at all (that dict is combat.rs's own theft loop).
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Spy"), 1);
        let p = &state.players[0];
        assert_eq!((p.food, p.resources, p.science, p.culture), (0, 0, 0, 0));
    }

    #[test]
    fn apply_gains_negative_sign_inverts_and_floors_at_zero() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.food = 1;
        p0.resources = 1;
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Aggression: Enslave"), -1);
        // gainFood/gainResources are both 2; losing floors at 0, not -1.
        assert_eq!(state.players[0].food, 0);
        assert_eq!(state.players[0].resources, 0);
    }

    #[test]
    fn apply_gains_decrease_population_enqueues_lose_pop() {
        // "Barbarians" prints a top-level `decreasePopulation: 1` (never
        // actually reached through apply_gains by Python either -- see this
        // module's top doc comment -- but the branch is real and tested here
        // directly).
        let p0 = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p0);
        apply_gains(&mut state, 0, card("Barbarians"), 1);
        assert_eq!(
            state.queue.pop_front(),
            Some(QueueItem::LosePop { player: 0, n: 1 })
        );
    }

    // ------------------------------------------------------- food_or_resources

    #[test]
    fn food_or_resources_gain_prefers_resources_when_blue_tokens_cover_it() {
        let mut p = blank_player(0, card("Despotism"));
        p.blue_total = 10;
        food_or_resources(&mut p, 5, 1);
        assert_eq!(p.resources, 5);
        assert_eq!(p.food, 0);
    }

    #[test]
    fn food_or_resources_gain_with_no_blue_tokens_grants_nothing() {
        // No blue tokens at all -- `gain_resources`/`gain_food` are each
        // capped by `blue_available`, which with `blue_total == 0` is 0, so
        // NEITHER can gain anything and the whole amount is dropped (both
        // draw from the SAME shared bank, so there is no separate "food
        // allowance" for the remainder to fall back on).
        let mut p = blank_player(0, card("Despotism"));
        food_or_resources(&mut p, 5, 1);
        assert_eq!(p.resources, 0);
        assert_eq!(p.food, 0);
    }

    #[test]
    fn food_or_resources_lose_drains_resources_before_food() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 3;
        p.food = 3;
        food_or_resources(&mut p, 5, -1);
        assert_eq!(p.resources, 0);
        assert_eq!(p.food, 1); // 3 resources covers 3 of the 5, food covers 2.
    }

    #[test]
    fn food_or_resources_lose_floors_at_zero() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 1;
        p.food = 1;
        food_or_resources(&mut p, 5, -1);
        assert_eq!(p.resources, 0);
        assert_eq!(p.food, 0);
    }

    // -------------------------------------------------------- final scoring

    /// Like `one_player_state`, but with `num_players` real seats (players
    /// beyond `players_in` are filled the same way `one_player_state` fills
    /// player 0's siblings) and a caller-supplied `current_events` list, for
    /// exercising [`final_event_awards`]/[`evaluate_final_events`], which
    /// read across every live player.
    fn multi_player_state(
        num_players: u8,
        players_in: &[PlayerState],
        current_events: &[CardId],
    ) -> GameState {
        let filler = || blank_player(9, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        for (i, p) in players_in.iter().enumerate() {
            players[i] = p.clone();
        }
        let mut ce = CardList::new();
        for &c in current_events {
            ce.push(c);
        }
        let mut state = one_player_state(players[0].clone());
        state.num_players = num_players;
        state.players = players;
        state.current_events = ce;
        state
    }

    #[test]
    fn pending_final_events_only_returns_scoring_event_cards() {
        // "Impact of Industry" (scoringEvent) is pending; an ordinary Age III
        // military card sitting in the same deck is not -- `pending_final_events`
        // must not treat every Age III card in `current_events` as scoring.
        let state = multi_player_state(
            2,
            &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))],
            &[card("Impact of Industry")],
        );
        assert_eq!(pending_final_events(&state), vec![card("Impact of Industry")]);
    }

    #[test]
    fn evaluate_final_events_scores_mine_resources_for_impact_of_industry() {
        // Bronze (a starting mine) prints 1 resource/worker; 2 workers on it
        // -> `mine_resources` = 2 -> "Impact of Industry"'s
        // `culturePerResourceProducedByMines: 1` awards 2 culture.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 2, stored: 0 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Industry")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 2);
        assert_eq!(state.players[1].culture, 0);
    }

    #[test]
    fn evaluate_final_events_applies_the_rankingculture_table_by_live_player_count() {
        // "Impact of Strength" ranks by strength; with nobody's tableau
        // contributing strength, ties break by turn order (start_player
        // first), so at 2p the table [10, 0] goes to seats 0 then 1.
        let p0 = blank_player(0, card("Despotism"));
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Strength")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 10);
        assert_eq!(state.players[1].culture, 0);
    }

    #[test]
    fn evaluate_final_events_clamps_negative_culture_at_zero_per_award() {
        // "Impact of Happiness" (`culturePerDiscontentWorker: -2`) can drive
        // a player's running culture negative; `evaluate_final_events` clamps
        // after EACH award, matching `p.culture = max(0, p.culture + amount)`.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 2, stored: 0 });
        p0.workers_free = 20; // plenty of unused workers to go discontent.
        p0.culture = 1;
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1], &[card("Impact of Happiness")]);
        evaluate_final_events(&mut state);
        assert_eq!(state.players[0].culture, 0);
    }

    #[test]
    fn scoring_culture_counts_completed_wonders_by_their_own_age() {
        // "Impact of Wonders": {A: 5, I: 4, II: 3, III: 2}.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.completed_wonders.push(card("Pyramids")); // Age A wonder -> 5.
        let block = final_scoring_block(card("Impact of Wonders")).unwrap();
        let state = one_player_state(p0.clone());
        assert_eq!(scoring_culture(&state, &p0, block), 5);
    }
}
