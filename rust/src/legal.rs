//! Legal move generation -- the port of `engine/actions.py`'s enumeration
//! half: `legal_moves`, `_politics_moves`, `_action_moves`, `_tableau`,
//! `_sorted_unique`, `_can_revolt`, `_action_card_playable`,
//! `free_action_moves`.
//!
//! Built on `costs.rs` (row/take/build/upgrade/develop costs and the take
//! gate) and `effects::state_stats`. Move ORDER is part of the differential-
//! test contract (DESIGN.md): the bots break ties by index, so every place
//! Python iterates `sorted(...)` over card NAMES, this sorts by
//! `CardId::name()` -- NOT by `CardId`'s own numeric order, which is
//! `card_table.rs` declaration order and has no relationship to Python's
//! alphabetical name order. See [`sorted_unique_into`].
//!
//! ## KNOWN GAPS (verified against the actual source/data 2026-08-05, not
//! oversights here -- reported to the coordinator; delivery failed on the
//! relay ["2 hops from a human" limit], so this doc comment is the durable
//! record until it is re-reported)
//!
//! 1. ~~`Card` has no `revolutionCost` field~~ **FIXED under this module
//!    while it was being written**: `cards.rs` grew `Card::revolution_cost`
//!    (and `peaceful_cost`, `stages`, `immediate_effects`) mid-flight, ahead
//!    of `costs.rs` being updated to use any of them. [`can_revolt`] reads
//!    `revolution_cost` directly -- it always did in Python too
//!    (`card.get("revolutionCost")` is read straight in `actions.py`, not
//!    through `costs.py`/`effects.py`), so this needed no `costs.rs` change
//!    and is ported in full, including the `free_action_moves`
//!    `develop_technology` revolt sub-case (Breakthrough, RB p.15).
//! 2. **Combat/pact resolution (a `combat.rs`) is not built yet.**
//!    `effects.rs` has no `pacts_for`/`pact_forbids_attack`/
//!    `attack_strength`/`defense_strength`/`war_forbidden`, and `Stats` has
//!    no `war_immune`. This blocks `offer_pact`/`aggression`/`war` move
//!    generation in [`politics_moves`] entirely (`Card` also has no "prints
//!    distinct A/B sides" flag, which would additionally block `offer_pact`
//!    even once combat.rs lands). `pol_pass`, `resign`, `prepare_event` and
//!    `cancel_pact` need none of that infrastructure and are fully ported --
//!    `cancel_pact` in particular works today because `PlayerState::pacts`
//!    already exists, it is only the ATTACK-side queries that are missing.
//!    `PlayerState` DOES now carry `war_declared_by_me`/`wars_declared_on_me`
//!    (state.rs grew them so `apply.rs::h_war` could be ported), so the one
//!    piece of `war` move generation that needed no combat math at all --
//!    "you may not declare a second war while one is already open"
//!    (`p.war_declared_by_me` truthy in Python's `_politics_moves`) -- is no
//!    longer blocked on a missing field, only on `war_forbidden` above.
//!
//! Two gaps this module used to carry forward from `costs.rs` are now
//! closed: this module used to derive a best-effort `taken_leader_ages`
//! bitmask from `p.leader`'s CURRENT age only, because `PlayerState` had no
//! real per-player history; `state.rs` grew `taken_leader_ages` and `costs::
//! take_gate`/`can_take` read it directly, so this module no longer computes
//! or passes one at all. And government develop cost used to be priced at 0
//! in [`action_moves`] (the `techCost`/`peacefulCost` gap in `costs::
//! tech_cost`); `Card::peaceful_cost` landed and `costs::tech_cost` reads it,
//! so `costs::tech_cost_net` now returns the real price for every
//! government. Wonder-stage moves are likewise no longer blocked:
//! `costs::wonder_stage_cost` reads `Card::stages` now, so [`action_moves`]
//! and [`free_action_moves`] both generate `WonderStep` moves.
//!
//! [`legal_moves`] also does not check a `state.pending` flag:
//! `engine/interact.py` (the decision-queue subsystem Python routes to for a
//! mid-resolution choice -- a colonization auction, a defense commitment, an
//! open `choice`) is not ported, and `state.rs` has no `pending` field at
//! all, so there is nothing to branch on yet.

use crate::card_table::FreeCivilActionValue;
use crate::cards::{Age, Card, CardId, CardType, Special};
use crate::costs;
use crate::economy;
use crate::effects;
use crate::moves::{ChurchillChoice, Move, MoveList};
use crate::state::{GameState, Pact, Phase, PlayerState, Tableau, MAX_HAND, MAX_TABLEAU};

// --------------------------------------------------------------- helpers

/// `sorted(set(items))`, by NAME -- not by `CardId`'s own numeric order,
/// which is `card_table.rs` declaration order and unrelated to Python's
/// alphabetical `sorted()` over card name strings. Move order is part of the
/// differential-test contract (DESIGN.md), so every place Python sorts a
/// collection of card names, this sorts by [`CardId::name`] too.
///
/// Mirrors `engine/actions.py::_sorted_unique`, minus the `@lru_cache`: see
/// this module's top doc comment on why nothing here is memoized (matching
/// `effects::state_stats`/`costs.rs`'s already-established choice). Writes
/// into `buf` (which must be at least `items.len()` long) and returns the
/// count written -- no allocation, matching this module's fixed-size-array
/// neighbours.
fn sorted_unique_into(items: &[CardId], buf: &mut [CardId]) -> usize {
    let mut n = 0;
    for &id in items {
        if !buf[..n].contains(&id) {
            buf[n] = id;
            n += 1;
        }
    }
    buf[..n].sort_unstable_by_key(|id| id.name());
    n
}

/// The name-sorted list of every card in `techs`. Mirrors the
/// `names_sorted` part of `engine/actions.py::_tableau`.
///
/// Python's `_tableau` additionally precomputes `by_type`/`urban_workers`/
/// `higher` as dicts, cached with `@lru_cache`. Neither is reproduced here:
/// `by_type`/`higher` are unnecessary -- filtering this one sorted array by
/// `CardId::kind()`/`CardId::level()` inline (see [`action_moves`]'s
/// build/upgrade loops) does the same job without a `HashMap` (DESIGN.md
/// rule 1), and `urban_workers` is exactly [`costs::urban_count`], already
/// ported. And per this module's top doc comment, nothing here is memoized,
/// matching `effects::state_stats`.
fn tableau_names_sorted(techs: &Tableau, buf: &mut [CardId; MAX_TABLEAU]) -> usize {
    let mut n = 0;
    for (id, _) in techs.iter() {
        buf[n] = id;
        n += 1;
    }
    buf[..n].sort_unstable_by_key(|id| id.name());
    n
}

/// Whether `p`'s active leader is the named leader. Duplicated from (not
/// imported from) `costs.rs::leader_is`, which is private to that module --
/// see costs.rs's own "a note on leader identity" for why this is a name
/// compare against `Card.name` rather than a lookup key (DESIGN.md rule 1 is
/// about keys, and no name here is ever used as one).
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

/// `engine/economy.py::pop_cost` -- the `state`/`p`-reading wrapper around
/// `economy::pop_food_cost`'s pure formula. Not in `economy.rs` itself:
/// that module predates `effects.rs` and deliberately left this one wrapper
/// for whichever module needed it first (see its own module doc "What is
/// NOT here, and why"). `one_time_food_discount` is always 0 here --
/// `PlayerState` has no `one_time_discount` field (events are not ported;
/// the same already-documented gap `costs.rs`'s `build_cost_for`/
/// `tech_cost` carry).
fn pop_cost(state: &GameState, p: &PlayerState) -> Option<i32> {
    let stats = effects::state_stats(state, p);
    economy::pop_food_cost(stats.pop_food_discount, p.yellow_bank, 0)
}

/// Every pact `idx` is party to, wherever it physically sits (§5.9). Mirrors
/// `engine/effects.py::pacts_for`. Self-contained -- unlike
/// `attack_strength`/`defense_strength`/`pact_forbids_attack`/
/// `war_forbidden` (blocked, see this module's top doc comment),
/// `PlayerState::pacts` already carries everything this needs.
fn pacts_for(state: &GameState, idx: u8) -> impl Iterator<Item = &Pact> {
    state.players[..state.num_players as usize]
        .iter()
        .flat_map(|q| q.pacts.as_slice().iter())
        .filter(move |pact| pact.is_party(idx))
}

// ------------------------------------------------------- move generation

/// The single source of truth for what a player may do right now. Mirrors
/// `engine/actions.py::legal_moves`.
pub fn legal_moves(state: &GameState) -> MoveList {
    if state.game_over {
        return MoveList::new();
    }
    // Python: `if state.pending: return interact.pending_moves(state)`.
    // Not checked here -- see this module's top doc comment: that whole
    // subsystem is not ported, and `state.rs` has no `pending` field to
    // branch on. This needs a branch here, before the phase dispatch below,
    // once it lands.
    let p = state.me();
    match state.phase {
        Phase::Politics => politics_moves(state, p),
        Phase::Actions => action_moves(state, p),
        // Python's `phase` is a plain string with three values ("politics",
        // "actions", "done"); `"done"` is only ever set after `game_over` is
        // already true (`engine/game.py:384`), which is handled above, so
        // this arm is unreachable in practice. Written out rather than
        // folded into a wildcard: DESIGN.md's recurring bug class is exactly
        // "a case silently falls through" -- if `Phase` ever grows a value
        // reachable before `game_over`, this must not quietly return empty.
        Phase::Done => MoveList::new(),
    }
}

/// Mirrors `engine/actions.py::_politics_moves`. See this module's top doc
/// comment (gap 3): `offer_pact`/`aggression`/`war` are not generated here,
/// combat/pact resolution does not exist yet.
fn politics_moves(state: &GameState, p: &PlayerState) -> MoveList {
    let mut moves = MoveList::new();
    moves.push(Move::PolPass);
    // Python: `if not state.has_military: return moves`. `has_military` is a
    // CARD-DATABASE-completeness flag (`db.has_military`,
    // `engine/cards.py:116`), not per-game progress -- it is only ever false
    // while loading a PARTIAL database. `card_table.rs` always compiles in
    // the full base game (236 cards, asserted by `lib.rs`'s own test), so
    // this is always true for this engine and the early return never fires;
    // there is no `state.has_military` field to read here.

    // §5.11 resign: not in age IV. ("Never the last player standing" is
    // enforced by the resignation/game-over logic outside move generation in
    // Python too -- `game.after_resign` -- not by `_politics_moves` itself.)
    if state.age_civil != Age::IV {
        moves.push(Move::Resign);
    }
    for pact in pacts_for(state, p.idx) {
        moves.push(Move::CancelPact { owner: pact.owner });
    }

    let mut buf = [CardId::NONE; MAX_HAND];
    let n = sorted_unique_into(p.hand_military.as_slice(), &mut buf);
    for &id in &buf[..n] {
        match id.kind() {
            CardType::Event | CardType::Territory => {
                moves.push(Move::PrepareEvent { card: id });
            }
            CardType::Pact => {
                // BLOCKED: `Card` carries no "prints distinct A/B sides"
                // flag (Python reads `card.get("sides")`), and even the
                // single-side case needs `state.num_players < 3` (pacts are
                // setup-removed at 2p) plus per-target enumeration -- see
                // this module's top doc comment, gap 3.
            }
            CardType::Aggression => {
                // BLOCKED: needs `effects::attack_strength`/
                // `defense_strength`/`pact_forbids_attack`, none of which
                // exist yet. See this module's top doc comment, gap 3.
            }
            CardType::War => {
                // BLOCKED: needs `effects::war_forbidden`
                // (`pact_forbids_attack` + `Stats::war_immune`, neither
                // exists yet). See this module's top doc comment, gap 3.
            }
            _ => {}
        }
    }
    moves
}

/// Mirrors `engine/actions.py::_action_moves`. See this module's top doc
/// comment for what is deliberately not generated here (wonder steps,
/// government revolution).
fn action_moves(state: &GameState, p: &PlayerState) -> MoveList {
    let mut moves = MoveList::new();
    moves.push(Move::EndTurn);
    let ca = costs::spare_ca(p);

    // take a card from the row
    let gate = costs::take_gate(state, p, None);
    for (idx, &id) in state.card_row.iter().enumerate() {
        if !id.is_none() && costs::can_take_gated(state, p, idx, &gate, Some(id)) {
            moves.push(Move::Take { slot: idx as u8 });
        }
    }

    if state.round == 1 {
        return moves; // §1.9: taking cards is the only legal action
    }

    let s = effects::state_stats(state, p);

    // increase population
    if let Some(cost) = pop_cost(state, p) {
        if ca >= 1 && p.food as i32 >= cost {
            moves.push(Move::Pop);
        }
    }
    if s.free_pop_per_turn && !p.ocean_liners_used && p.yellow_bank > 0 {
        moves.push(Move::PopFree);
    }

    // --- loop invariant: the name-sorted tableau, computed once (see
    // `tableau_names_sorted`'s doc comment for why no `by_type`/`higher`/
    // `urban_workers` maps are precomputed alongside it).
    let mut names_buf = [CardId::NONE; MAX_TABLEAU];
    let names_n = tableau_names_sorted(&p.techs, &mut names_buf);
    let names = &names_buf[..names_n];

    let have_ma = p.military_actions >= 1;
    let have_ca = ca >= 1;
    let res = p.resources as i32;
    let disc = p.mil_discount as i32;

    // build
    if p.workers_free > 0 {
        for id in names.iter().copied().filter(|id| id.kind().takes_workers()) {
            let kind = id.kind();
            let cost = match costs::build_cost_for(state, p, id) {
                Some(c) => c,
                None => continue,
            };
            if kind.is_unit() {
                if res < (cost - disc).max(0) || !have_ma {
                    continue;
                }
            } else {
                if res < cost || !have_ca {
                    continue;
                }
                if kind.is_urban() && costs::urban_count(p, kind) >= s.urban_limit {
                    continue;
                }
            }
            moves.push(Move::Build { card: id });
        }
    }

    // upgrade
    for lo in names.iter().copied().filter(|id| id.kind().takes_workers()) {
        if p.techs.workers(lo) == 0 {
            continue;
        }
        let lo_kind = lo.kind();
        let unit = lo_kind.is_unit();
        if unit {
            if !have_ma {
                continue;
            }
        } else if !have_ca {
            continue;
        }
        let lo_cost = costs::build_cost_for(state, p, lo).unwrap_or(0);
        let higher = names
            .iter()
            .copied()
            .filter(|id| id.kind() == lo_kind && *id != lo && id.level() > lo.level());
        for hi in higher {
            let hi_cost = costs::build_cost_for(state, p, hi).unwrap_or(0);
            let mut cost = (hi_cost - lo_cost).max(0);
            if unit {
                cost = (cost - disc).max(0);
            }
            if res >= cost {
                moves.push(Move::Upgrade { from: lo, to: hi });
            }
        }
    }

    // destroy / disband (§3.6, §4.3)
    for id in names.iter().copied() {
        if p.techs.workers(id) == 0 {
            continue;
        }
        if id.kind().is_unit() {
            if have_ma {
                moves.push(Move::Destroy { card: id });
            }
        } else if have_ca && id.kind().takes_workers() {
            moves.push(Move::Destroy { card: id });
        }
    }

    // wonder stages
    if !p.wonder.is_none() {
        let stages_left = p.wonder.get().stages.len() as i32 - p.wonder_steps as i32;
        if ca >= 1 {
            let max_k = stages_left.min(s.wonder_stages);
            for k in 1..=max_k {
                if p.resources as i32 >= costs::wonder_stage_cost(state, p, k as u8) {
                    moves.push(Move::WonderStep { steps: k as u8 });
                }
            }
        }
    }

    // hand: leaders, technologies, governments, action cards
    let mut hand_buf = [CardId::NONE; MAX_HAND];
    let hand_n = sorted_unique_into(p.hand_civil.as_slice(), &mut hand_buf);
    for &id in &hand_buf[..hand_n] {
        match id.kind() {
            CardType::Leader => {
                if ca >= 1 {
                    moves.push(Move::PlayLeader { card: id });
                }
            }
            CardType::Government => {
                // `costs::tech_cost` now reads `Card::peaceful_cost` for
                // governments, so this prices a peaceful develop at its real
                // science cost (`.unwrap_or(0)` only fires for Despotism,
                // which prints no `peacefulCost` at all and is never in a
                // hand to begin with).
                if ca >= 1 && p.science as i32 >= costs::tech_cost_net(state, p, id).unwrap_or(0) {
                    moves.push(Move::Develop { card: id });
                }
                if can_revolt(state, p, id) {
                    moves.push(Move::Revolution { card: id });
                }
            }
            CardType::Action => {
                // §3.11: not in the Action Phase it was TAKEN in. Counted,
                // not tested by name: a second copy taken this turn must not
                // lock up the copy that was already in hand.
                let held = p.hand_civil.as_slice().iter().filter(|&&c| c == id).count();
                let taken = p.taken_this_turn.as_slice().iter().filter(|&&c| c == id).count();
                if ca >= 1 && held > taken && action_card_playable(state, p, id) {
                    moves.push(Move::PlayAction { card: id });
                }
            }
            k if k.takes_workers() || k == CardType::SpecialTech => {
                if ca >= 1 && p.science as i32 >= costs::tech_cost_net(state, p, id).unwrap_or(0) {
                    moves.push(Move::Develop { card: id });
                }
            }
            _ => {} // Wonder/other military-deck types never sit in hand_civil.
        }
    }

    // tactics -- `state.has_military` always true, see `politics_moves`.
    if !p.tactic_action_used {
        if p.military_actions >= 1 {
            let mut tbuf = [CardId::NONE; MAX_HAND];
            let tn = sorted_unique_into(p.hand_military.as_slice(), &mut tbuf);
            for &id in &tbuf[..tn] {
                if id.kind() == CardType::Tactic {
                    moves.push(Move::PlayTactic { card: id });
                }
            }
        }
        if p.military_actions >= 2 {
            let mut abuf = [CardId::NONE; 16]; // available_tactics: CardList<16>
            let an = sorted_unique_into(state.available_tactics.as_slice(), &mut abuf);
            for &id in &abuf[..an] {
                if id != p.tactic {
                    moves.push(Move::CopyTactic { card: id });
                }
            }
        }
    }

    // Churchill's once-per-turn choice.
    if leader_is(p, "Winston Churchill") && !p.churchill_used {
        moves.push(Move::Churchill { choice: ChurchillChoice::Culture });
        moves.push(Move::Churchill { choice: ChurchillChoice::Military });
    }

    moves
}

/// §8.3 peaceful revolution -- and §8.3.4's Robespierre variant, which pays
/// with military actions instead of civil ones. Mirrors
/// `engine/actions.py::_can_revolt`. `revolution_cost == 0` means "not
/// printed" (matching every other zero-means-absent cost field in this
/// codebase, e.g. `costs::build_cost_for`'s `resource_cost == 0` -- verified
/// 2026-08-05 against `data/cards_civil.json`: every government that DOES
/// print a `revolutionCost` prints a nonzero one, 1 through 9; only
/// Despotism prints `null`).
fn can_revolt(state: &GameState, p: &PlayerState, id: CardId) -> bool {
    let card = id.get();
    if card.revolution_cost == 0 || (p.science as i32) < card.revolution_cost as i32 {
        return false;
    }
    if leader_is(p, "Maximilien Robespierre") {
        let s = effects::state_stats(state, p);
        return p.military_actions as i32 == s.military_actions && s.military_actions > 0;
    }
    p.civil_actions as i32 == costs::ca_total(state, p) && p.civil_actions > 0
}

/// §3.11: a yellow card that orders an action needs that action to be legal.
///
/// Mirrors `engine/actions.py::_action_card_playable`. For the 18 cards that
/// print an ordered action (`Special::FreeCivilAction`, carrying a real
/// `FreeCivilActionValue` payload -- gen_cards.py, 2026-08-05), this maps the
/// payload onto [`FreeActionKind`] via [`free_action_kind_of`] and mirrors
/// Python's `bool(free_action_moves(state, p, kind, ...))` exactly -- legality
/// here never needs to know whether applying the move would additionally
/// need `interact.rs`'s decision queue (that only matters to `apply.rs`,
/// once it knows WHICH single move to run). Cards with no ordered action are
/// unaffected -- that branch only needs `card.effects`/`card.special`, all of
/// which exist, and is ported in full via [`action_card_has_any_gain`].
fn action_card_playable(state: &GameState, p: &PlayerState, id: CardId) -> bool {
    let card = id.get();
    if let Some(value) = card.special.iter().find_map(|s| match s {
        Special::FreeCivilAction(v) => Some(*v),
        _ => None,
    }) {
        let kind = free_action_kind_of(value);
        // RB p.15: Breakthrough may spend its order on a revolution instead,
        // which needs every civil action still unspent -- see
        // `free_action_moves`'s `DevelopTechnology` arm.
        let revolt_ok = p.civil_actions as i32 == costs::ca_total(state, p);
        let discount = card.effects.resource_discount as i32;
        return !free_action_moves(state, p, kind, discount, revolt_ok).is_empty();
    }
    action_card_has_any_gain(card)
}

/// Whether `card` has at least one immediate gain effect (§3.11: "cards with
/// no ordered action gain immediately"). Mirrors Python's
/// `any(k in ACTION_CARD_KEYS for k in eff)`.
///
/// `ACTION_CARD_KEYS` also lists `gainPopulation`/`extraCivilActions`/
/// `extraMilitaryActions`; verified (2026-08-05, all of `data/*.json`) that
/// none of the three is ever printed on any action card in the base game, so
/// their absence from this check changes nothing observable -- not a routed-
/// around gap, an accurate statement about the data.
fn action_card_has_any_gain(card: &Card) -> bool {
    let eff = &card.effects;
    if eff.gain_science != 0
        || eff.gain_culture != 0
        || eff.gain_food != 0
        || eff.gain_resources != 0
        || eff.military_actions != 0
        || eff.resources_for_military_units != 0
    {
        return true;
    }
    card.special.iter().any(|s| {
        matches!(
            s,
            Special::GainFoodOrResources(_)
                | Special::ResourcesForMilitaryUnitsPerStrongerCivilization
                | Special::CulturePerCivilizationWithMoreCulture
        )
    })
}

// ------------------------------------------------- ordered (free) actions

/// The kind of ordered action a yellow action card grants (§3.11). Mirrors
/// the six string values `data/*.json` prints under `effects.freeCivilAction`
/// -- a second, independently-named enum from `card_table.rs`'s
/// `FreeCivilActionValue` (which mirrors the six JSON strings directly, the
/// way every other generated `<Key>Value` enum does) rather than the SAME
/// enum, because this one additionally has to be the argument shape
/// `free_action_moves` matches on, which predates the payload existing at
/// all. [`free_action_kind_of`] is the one place that maps between them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreeActionKind {
    IncreasePopulation,
    BuildOneWonderStage,
    DevelopTechnology,
    BuildOrUpgradeFarmOrMine,
    BuildOrUpgradeUrbanBuilding,
    UpgradeFarmMineOrUrbanBuilding,
}

/// Maps `card_table.rs`'s generated `FreeCivilActionValue` (what
/// `Special::FreeCivilAction` carries) onto this module's own
/// `FreeActionKind` (what [`free_action_moves`] matches on). THE one place
/// that equivalence is asserted -- `apply.rs::h_play_action` calls this too
/// rather than repeating the match, for the same reason `fixtures.rs::
/// parse_move` is documented as the one place a Python move-tag string maps
/// to a `Move` variant: two independently-maintained copies of "these two
/// enums mean the same six things" is exactly the registry-drift bug class
/// DESIGN.md's `Special` enum exists to close, and a hand-written `match`
/// here is not exempt from that just because both enums are hand-written too.
pub fn free_action_kind_of(v: FreeCivilActionValue) -> FreeActionKind {
    use FreeCivilActionValue as V;
    use FreeActionKind as K;
    match v {
        V::BuildOrUpgradeFarmOrMine => K::BuildOrUpgradeFarmOrMine,
        V::BuildOrUpgradeUrbanBuilding => K::BuildOrUpgradeUrbanBuilding,
        V::IncreasePopulation => K::IncreasePopulation,
        V::BuildOneWonderStage => K::BuildOneWonderStage,
        V::DevelopTechnology => K::DevelopTechnology,
        V::UpgradeFarmMineOrUrbanBuilding => K::UpgradeFarmMineOrUrbanBuilding,
    }
}

/// Whether `kind` accepts cards of type `k` for its build/upgrade half.
/// Mirrors Python's `_FREE_BUILD_TYPES` dict; `false` for `kind`s that are
/// not one of the three build/upgrade kinds at all.
fn free_build_types(kind: FreeActionKind, k: CardType) -> bool {
    use FreeActionKind::*;
    match kind {
        BuildOrUpgradeFarmOrMine => k.is_production(),
        BuildOrUpgradeUrbanBuilding => k.is_urban(),
        UpgradeFarmMineOrUrbanBuilding => k.is_production() || k.is_urban(),
        IncreasePopulation | BuildOneWonderStage | DevelopTechnology => false,
    }
}

/// Concrete moves satisfying an action card's ordered action (§3.11). Mirrors
/// `engine/actions.py::free_action_moves`. The action is performed under
/// normal rules but pays no civil/military action, and `discount` resources
/// come off its cost (floored at 0).
pub fn free_action_moves(
    state: &GameState,
    p: &PlayerState,
    kind: FreeActionKind,
    discount: i32,
    revolt_ok: bool,
) -> MoveList {
    use FreeActionKind::*;
    let mut out = MoveList::new();
    match kind {
        IncreasePopulation => {
            // At full price -- Python's `increase_population` branch does
            // not apply `discount` here either (only the build/upgrade/
            // wonder-step kinds do).
            if let Some(cost) = pop_cost(state, p) {
                if p.food as i32 >= cost {
                    out.push(Move::Pop);
                }
            }
        }
        BuildOneWonderStage => {
            if !p.wonder.is_none() && (p.wonder_steps as usize) < p.wonder.get().stages.len() {
                let cost = (costs::wonder_stage_cost(state, p, 1) - discount).max(0);
                if p.resources as i32 >= cost {
                    out.push(Move::WonderStep { steps: 1 });
                }
            }
        }
        DevelopTechnology => {
            let mut buf = [CardId::NONE; MAX_HAND];
            let n = sorted_unique_into(p.hand_civil.as_slice(), &mut buf);
            for &id in &buf[..n] {
                let card = id.get();
                if !card.kind.is_developable() {
                    continue;
                }
                if p.science as i32 >= costs::tech_cost_net(state, p, id).unwrap_or(0) {
                    out.push(Move::Develop { card: id });
                }
                // RB p.15: Breakthrough may also pay for a revolution --
                // this is `_can_revolt`'s COST test only (`revolution_cost`
                // printed and affordable), deliberately NOT `can_revolt`
                // itself: that also requires every civil/military action
                // this turn to be unspent, which is not true while
                // Breakthrough's own order is resolving (its `1 CA` has
                // already been spent taking the ordered-action branch).
                // `revolt_ok` is the caller's pre-computed answer to that
                // part, matching Python's `_action_card_playable` passing
                // `p.civil_actions == ca_total(state, p)` in as a bool.
                if revolt_ok && card.kind == CardType::Government {
                    let rc = card.revolution_cost as i32;
                    if rc != 0 && p.science as i32 >= rc {
                        out.push(Move::Revolution { card: id });
                    }
                }
            }
        }
        BuildOrUpgradeFarmOrMine | BuildOrUpgradeUrbanBuilding | UpgradeFarmMineOrUrbanBuilding => {
            let upgrade_only = kind == UpgradeFarmMineOrUrbanBuilding;
            let s = effects::state_stats(state, p);
            let mut buf = [CardId::NONE; MAX_TABLEAU];
            let n = tableau_names_sorted(&p.techs, &mut buf);
            let names = &buf[..n];

            if !upgrade_only && p.workers_free > 0 {
                for id in names.iter().copied().filter(|id| free_build_types(kind, id.kind())) {
                    let k = id.kind();
                    let cost = match costs::build_cost_for(state, p, id) {
                        Some(c) => c,
                        None => continue,
                    };
                    if (p.resources as i32) < (cost - discount).max(0) {
                        continue;
                    }
                    if k.is_urban() && costs::urban_count(p, k) >= s.urban_limit {
                        continue;
                    }
                    out.push(Move::Build { card: id });
                }
            }
            for lo in names.iter().copied().filter(|id| free_build_types(kind, id.kind())) {
                if p.techs.workers(lo) == 0 {
                    continue;
                }
                let lo_kind = lo.kind();
                let higher = names
                    .iter()
                    .copied()
                    .filter(|id| id.kind() == lo_kind && *id != lo && id.level() > lo.level());
                for hi in higher {
                    let cost = costs::upgrade_cost(state, p, lo, hi);
                    if (p.resources as i32) >= (cost - discount).max(0) {
                        out.push(Move::Upgrade { from: lo, to: hi });
                    }
                }
            }
        }
    }
    out
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Age;
    use crate::state::{CardList, GameState, PactList, Phase, PlayerState, Tableau, TechSlot, MAX_PLAYERS, ROW_SIZE};

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
            resigned: false,
            taken_leader_ages: 0,
            war_declared_by_me: CardId::NONE,
            war_target: 0,
            wars_declared_on_me: [CardId::NONE; MAX_PLAYERS],
        }
    }

    fn blank_state(num_players: u8, players: [PlayerState; MAX_PLAYERS]) -> GameState {
        GameState {
            num_players,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 2, // most tests want round > 1 so the action phase is not truncated
            start_player: 0,
            age_civil: Age::A,
            age_military: Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: Age::A,
            scoring_events: CardList::new(),
            available_tactics: CardList::new(),
            civil_discard: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
            civil_removed: [
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
                CardList::new(),
            ],
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
        }
    }

    fn one_player_state(p: PlayerState) -> GameState {
        let filler = || blank_player(0, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p;
        blank_state(4, players)
    }

    // ------------------------------------------------------- sorted_unique_into

    #[test]
    fn sorted_unique_into_dedupes_and_sorts_by_name() {
        let items = [card("Irrigation"), card("Bronze"), card("Irrigation")];
        let mut buf = [CardId::NONE; 8];
        let n = sorted_unique_into(&items, &mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf[0], card("Bronze"), "alphabetically first");
        assert_eq!(buf[1], card("Irrigation"));
    }

    // ---------------------------------------------------- tableau_names_sorted

    #[test]
    fn tableau_names_sorted_is_name_order_not_insertion_order() {
        let mut techs = Tableau::new();
        // Insert in an order that is NOT alphabetical.
        techs.insert(card("Irrigation"), TechSlot { workers: 1, stored: 0 });
        techs.insert(card("Bronze"), TechSlot { workers: 1, stored: 0 });
        let mut buf = [CardId::NONE; MAX_TABLEAU];
        let n = tableau_names_sorted(&techs, &mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf[0], card("Bronze"));
        assert_eq!(buf[1], card("Irrigation"));
    }

    // ------------------------------------------------------- taken_leader_ages

    #[test]
    fn action_moves_take_reads_taken_leader_ages_off_the_player_directly() {
        // Unlike the old best-effort derivation from `p.leader`'s CURRENT
        // age, `p.taken_leader_ages` is real per-player history now: a
        // player who took (and has since replaced) an Age-A leader must
        // still be blocked from taking a SECOND Age-A leader, even though
        // `p.leader` no longer points at the first one.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let leader_slot = card("Napoleon Bonaparte"); // Age A
        let age_bit = 1u8 << (leader_slot.get().age as u8);
        p.taken_leader_ages = age_bit;
        p.leader = card("Hammurabi"); // a DIFFERENT (non-Age-A) leader now held
        let mut state = one_player_state(p);
        state.card_row[0] = leader_slot;
        let moves = action_moves(&state, &state.players[0]);
        assert!(
            !moves.as_slice().iter().any(|m| matches!(m, Move::Take { slot: 0 })),
            "that age's leader was already taken this game, regardless of who is currently held"
        );
    }

    // -------------------------------------------------------------- legal_moves

    #[test]
    fn legal_moves_is_empty_when_game_over() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        state.game_over = true;
        assert!(legal_moves(&state).is_empty());
    }

    #[test]
    fn legal_moves_is_empty_in_done_phase() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        state.phase = Phase::Done;
        assert!(legal_moves(&state).is_empty(), "unreachable in practice, but must not silently misbehave");
    }

    #[test]
    fn legal_moves_dispatches_on_phase() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        state.phase = Phase::Politics;
        let politics = legal_moves(&state);
        assert!(politics.as_slice().contains(&Move::PolPass));
        assert!(!politics.as_slice().iter().any(|m| matches!(m, Move::EndTurn)));

        state.phase = Phase::Actions;
        let actions = legal_moves(&state);
        assert!(actions.as_slice().contains(&Move::EndTurn));
        assert!(!actions.as_slice().iter().any(|m| matches!(m, Move::PolPass)));
    }

    // ----------------------------------------------------------- politics_moves

    #[test]
    fn politics_pass_is_always_legal() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        let moves = politics_moves(&state, &state.players[0]);
        assert_eq!(moves.as_slice()[0], Move::PolPass);
    }

    #[test]
    fn politics_resign_gated_on_not_age_iv() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        assert!(politics_moves(&state, &state.players[0]).as_slice().contains(&Move::Resign));
        state.age_civil = Age::IV;
        assert!(!politics_moves(&state, &state.players[0]).as_slice().contains(&Move::Resign));
    }

    #[test]
    fn politics_prepare_event_from_hand_military_event_or_territory_cards() {
        let mut p = blank_player(0, card("Despotism"));
        // Pick two real event/territory cards from the table.
        let event = CardId::by_name("Great Fire")
            .or_else(|| crate::cards::CARDS.iter().position(|c| c.kind == CardType::Event).map(|i| CardId(i as u16)))
            .unwrap();
        p.hand_military.push(event);
        let state = one_player_state(p);
        let moves = politics_moves(&state, &state.players[0]);
        assert!(moves.as_slice().contains(&Move::PrepareEvent { card: event }));
    }

    #[test]
    fn politics_cancel_pact_reads_pacts_the_player_is_party_to() {
        let mut p = blank_player(0, card("Despotism"));
        let pact_card = crate::cards::CARDS
            .iter()
            .position(|c| c.kind == CardType::Pact)
            .map(|i| CardId(i as u16))
            .expect("at least one pact card exists");
        p.pacts.push(Pact { card: pact_card, owner: 0, partner: 1, a: 0, b: 1 });
        let state = one_player_state(p);
        let moves = politics_moves(&state, &state.players[0]);
        assert!(moves.as_slice().contains(&Move::CancelPact { owner: 0 }));
    }

    #[test]
    fn politics_never_generates_aggression_war_or_offer_pact_yet() {
        // Pin for the documented gap (combat.rs not built): even with a
        // military-deck hand full of aggression/war/pact cards, none of
        // those three move kinds is ever produced today.
        let mut p = blank_player(0, card("Despotism"));
        for c in crate::cards::CARDS.iter().enumerate() {
            let (i, card) = c;
            if matches!(card.kind, CardType::Aggression | CardType::War | CardType::Pact) {
                if p.hand_military.len() < 24 {
                    p.hand_military.push(CardId(i as u16));
                }
            }
        }
        let state = one_player_state(p);
        let moves = politics_moves(&state, &state.players[0]);
        assert!(!moves.as_slice().iter().any(|m| matches!(m, Move::Aggression { .. } | Move::War { .. } | Move::OfferPact { .. })));
    }

    // ------------------------------------------------------------ action_moves

    #[test]
    fn round_one_only_offers_end_turn_and_take() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        state.round = 1;
        state.card_row[0] = card("Selective Breeding");
        let moves = legal_moves(&state);
        for m in moves.as_slice() {
            assert!(matches!(m, Move::EndTurn | Move::Take { .. }), "{m:?} should not be legal in round 1");
        }
        assert!(moves.as_slice().contains(&Move::Take { slot: 0 }));
    }

    #[test]
    fn take_moves_follow_row_slot_order() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 10;
        let mut state = one_player_state(p);
        state.card_row[3] = card("Selective Breeding");
        state.card_row[1] = card("Irrigation");
        let moves = action_moves(&state, &state.players[0]);
        let takes: Vec<u8> = moves
            .as_slice()
            .iter()
            .filter_map(|m| if let Move::Take { slot } = m { Some(*slot) } else { None })
            .collect();
        assert_eq!(takes, vec![1, 3], "row order, not name order");
    }

    #[test]
    fn pop_move_gated_on_civil_action_and_food() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 20; // pop_cost_base -> 2
        p.food = 2;
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::Pop));

        let mut p2 = blank_player(0, card("Despotism"));
        p2.civil_actions = 4;
        p2.yellow_bank = 20;
        p2.food = 1; // one short
        let state2 = one_player_state(p2);
        assert!(!action_moves(&state2, &state2.players[0]).as_slice().contains(&Move::Pop));
    }

    #[test]
    fn build_move_needs_a_free_worker_and_enough_resources_and_civil_action() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.workers_free = 1;
        p.resources = 10;
        p.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        // Bronze is already in the tableau (0 workers); it must be a
        // candidate for BUILD (adding a worker), not develop/take.
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::Build { card: card("Bronze") }));

        let mut p2 = blank_player(0, card("Despotism"));
        p2.civil_actions = 4;
        p2.workers_free = 0; // no free worker
        p2.resources = 10;
        p2.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        let state2 = one_player_state(p2);
        assert!(!action_moves(&state2, &state2.players[0]).as_slice().contains(&Move::Build { card: card("Bronze") }));
    }

    #[test]
    fn upgrade_move_from_agriculture_to_irrigation() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        // Both ends of the upgrade must already be IN the tableau -- you can
        // only place a worker on a technology you have developed/taken, so
        // "upgrade" moves a worker between two cards the player already
        // owns, never to an undeveloped one (§3.7).
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0])
            .as_slice()
            .contains(&Move::Upgrade { from: card("Agriculture"), to: card("Irrigation") }));
    }

    #[test]
    fn destroy_move_needs_a_worker_present_and_the_matching_action_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.techs.insert(card("Bronze"), TechSlot { workers: 1, stored: 0 });
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::Destroy { card: card("Bronze") }));
    }

    #[test]
    fn wonder_step_generated_up_to_the_wonder_stages_limit_when_affordable() {
        // Pyramids: stages [3, 2, 1]. Default `Stats::wonder_stages` is 1 (no
        // card raises it), so with plenty of resources exactly ONE
        // `WonderStep { steps: 1 }` move should appear -- not `steps: 2` or
        // `steps: 3`, which would need a higher `wonder_stages` stat.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 100;
        p.wonder = card("Pyramids");
        let state = one_player_state(p);
        let moves = action_moves(&state, &state.players[0]);
        assert!(moves.as_slice().contains(&Move::WonderStep { steps: 1 }));
        assert!(!moves.as_slice().iter().any(|m| matches!(m, Move::WonderStep { steps } if *steps != 1)));
    }

    #[test]
    fn wonder_step_needs_enough_resources_and_a_civil_action() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 2; // Pyramids' first stage costs 3
        p.wonder = card("Pyramids");
        let state = one_player_state(p);
        assert!(!action_moves(&state, &state.players[0])
            .as_slice()
            .iter()
            .any(|m| matches!(m, Move::WonderStep { .. })));

        let mut p2 = blank_player(0, card("Despotism"));
        p2.civil_actions = 0; // no civil action left
        p2.resources = 100;
        p2.wonder = card("Pyramids");
        let state2 = one_player_state(p2);
        assert!(!action_moves(&state2, &state2.players[0])
            .as_slice()
            .iter()
            .any(|m| matches!(m, Move::WonderStep { .. })));
    }

    #[test]
    fn develop_technology_from_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 10;
        p.hand_civil.push(card("Irrigation"));
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::Develop { card: card("Irrigation") }));
    }

    #[test]
    fn develop_government_is_priced_at_its_real_peaceful_cost() {
        // Monarchy's peacefulCost is 8 (data/cards_civil.json).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 7; // one short
        p.hand_civil.push(card("Monarchy"));
        let state = one_player_state(p);
        assert!(
            !action_moves(&state, &state.players[0]).as_slice().contains(&Move::Develop { card: card("Monarchy") }),
            "7 science is not enough to peacefully develop Monarchy (needs 8)"
        );

        let mut p2 = blank_player(0, card("Despotism"));
        p2.civil_actions = 4;
        p2.science = 8;
        p2.hand_civil.push(card("Monarchy"));
        let state2 = one_player_state(p2);
        assert!(action_moves(&state2, &state2.players[0]).as_slice().contains(&Move::Develop { card: card("Monarchy") }));
    }

    #[test]
    fn revolution_move_needs_every_civil_action_unspent_and_enough_science() {
        // Monarchy's revolutionCost is 2 (data/cards_civil.json).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4; // == ca_total(Despotism) == 4: every CA unspent
        p.science = 2;
        p.hand_civil.push(card("Monarchy"));
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::Revolution { card: card("Monarchy") }));

        let mut p2 = blank_player(0, card("Despotism"));
        p2.civil_actions = 3; // one CA already spent -- no longer eligible
        p2.science = 2;
        p2.hand_civil.push(card("Monarchy"));
        let state2 = one_player_state(p2);
        assert!(!action_moves(&state2, &state2.players[0]).as_slice().iter().any(|m| matches!(m, Move::Revolution { .. })));
    }

    #[test]
    fn play_leader_from_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Napoleon Bonaparte"));
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::PlayLeader { card: card("Napoleon Bonaparte") }));
    }

    #[test]
    fn play_action_needs_an_untaken_copy_and_ca_and_playability() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        // "Frugality (A)": ordered action increase_population + gainFood.
        // `p.yellow_bank` is 0 here (blank_player default), so
        // `free_action_moves` has no legal `Move::Pop` to offer -- correctly
        // NOT playable in THIS position, not blocked outright (see
        // `play_action_playable_when_its_ordered_action_has_a_legal_move`
        // below for the positive case).
        p.hand_civil.push(card("Frugality (A)"));
        let state = one_player_state(p);
        assert!(
            !action_moves(&state, &state.players[0]).as_slice().contains(&Move::PlayAction { card: card("Frugality (A)") }),
            "no legal ordered-action move in this position"
        );
    }

    #[test]
    fn play_action_playable_when_its_ordered_action_has_a_legal_move() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5; // prices increase_population at 5, affordable below
        p.food = 5;
        p.hand_civil.push(card("Frugality (A)"));
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::PlayAction { card: card("Frugality (A)") }));
    }

    #[test]
    fn play_action_playable_for_a_pure_gain_card() {
        // "Impact of Variety" and friends aren't action-typed; find a real
        // action card with a gain key and no ordered action.
        let name = crate::cards::CARDS
            .iter()
            .find(|c| {
                c.kind == CardType::Action
                    && !c.special.iter().any(|s| matches!(s, Special::FreeCivilAction(_)))
                    && (c.effects.gain_science != 0
                        || c.effects.gain_culture != 0
                        || c.effects.gain_food != 0
                        || c.effects.gain_resources != 0)
            })
            .map(|c| c.name)
            .expect("at least one plain-gain action card exists");
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card(name));
        let state = one_player_state(p);
        assert!(action_moves(&state, &state.players[0]).as_slice().contains(&Move::PlayAction { card: card(name) }));
    }

    #[test]
    fn play_tactic_and_copy_tactic_gated_on_military_actions() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 2;
        let tactic = crate::cards::CARDS
            .iter()
            .position(|c| c.kind == CardType::Tactic)
            .map(|i| CardId(i as u16))
            .expect("at least one tactic exists");
        p.hand_military.push(tactic);
        let mut state = one_player_state(p);
        state.available_tactics.push(tactic);
        let moves = action_moves(&state, &state.players[0]);
        assert!(moves.as_slice().contains(&Move::PlayTactic { card: tactic }));
        // CopyTactic excludes the player's OWN current tactic.
        assert!(moves.as_slice().contains(&Move::CopyTactic { card: tactic }));
    }

    #[test]
    fn churchill_offers_both_choices_once_per_turn() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.leader = card("Winston Churchill");
        let state = one_player_state(p);
        let moves = action_moves(&state, &state.players[0]);
        assert!(moves.as_slice().contains(&Move::Churchill { choice: ChurchillChoice::Culture }));
        assert!(moves.as_slice().contains(&Move::Churchill { choice: ChurchillChoice::Military }));

        let mut p2 = blank_player(0, card("Despotism"));
        p2.civil_actions = 4;
        p2.leader = card("Winston Churchill");
        p2.churchill_used = true;
        let state2 = one_player_state(p2);
        assert!(!action_moves(&state2, &state2.players[0]).as_slice().iter().any(|m| matches!(m, Move::Churchill { .. })));
    }

    // -------------------------------------------------------- action_card_playable

    #[test]
    fn action_card_has_any_gain_false_for_an_ordered_action_with_no_gain_key() {
        // "Rich Land (A)": build_or_upgrade_farm_or_mine + resourceDiscount
        // only -- no gainScience/gainCulture/gainFood/gainResources/
        // militaryActions/resourcesForMilitaryUnits key at all (verified
        // 2026-08-05 against every action card in data/*.json: no action
        // card has EITHER no ordered action AND no gain key, so this is the
        // closest real card to "no gain keys" the base game has).
        let c = card("Rich Land (A)").get();
        assert!(!action_card_has_any_gain(c));
    }

    #[test]
    fn action_card_playable_false_for_an_ordered_action_with_no_legal_move() {
        // A blank player has no workers, no tableau and no resources: none
        // of the six ordered-action kinds has anything to offer, so this
        // must read as unplayable -- correctly, not because the kind is
        // unrecognised (see the six `action_card_playable_true_for_*` tests
        // below for the positive case of each kind).
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        assert!(!action_card_playable(&state, &state.players[0], card("Rich Land (A)")));
    }

    #[test]
    fn free_action_kind_of_maps_every_free_civil_action_value() {
        // THE one place `FreeCivilActionValue` (card_table.rs, generated)
        // maps onto `FreeActionKind` (this module, hand-written) -- assert
        // the six pairings directly so a mismatch fails here, not as a
        // mysterious "card X is never playable" symptom three modules away.
        use crate::card_table::FreeCivilActionValue as V;
        assert_eq!(free_action_kind_of(V::BuildOrUpgradeFarmOrMine), FreeActionKind::BuildOrUpgradeFarmOrMine);
        assert_eq!(free_action_kind_of(V::BuildOrUpgradeUrbanBuilding), FreeActionKind::BuildOrUpgradeUrbanBuilding);
        assert_eq!(free_action_kind_of(V::IncreasePopulation), FreeActionKind::IncreasePopulation);
        assert_eq!(free_action_kind_of(V::BuildOneWonderStage), FreeActionKind::BuildOneWonderStage);
        assert_eq!(free_action_kind_of(V::DevelopTechnology), FreeActionKind::DevelopTechnology);
        assert_eq!(
            free_action_kind_of(V::UpgradeFarmMineOrUrbanBuilding),
            FreeActionKind::UpgradeFarmMineOrUrbanBuilding
        );
    }

    #[test]
    fn action_card_playable_true_for_build_or_upgrade_farm_or_mine() {
        let mut p = blank_player(0, card("Despotism"));
        p.workers_free = 1;
        p.resources = 1; // Bronze costs 2, Rich Land's resourceDiscount is 1
        p.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert!(action_card_playable(&state, &state.players[0], card("Rich Land (A)")));
    }

    #[test]
    fn action_card_playable_true_for_build_or_upgrade_urban_building() {
        let mut p = blank_player(0, card("Despotism"));
        p.workers_free = 1;
        p.resources = 2; // Religion costs 3, Urban Growth's resourceDiscount is 1
        p.techs.insert(card("Religion"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert!(action_card_playable(&state, &state.players[0], card("Urban Growth (A)")));
    }

    #[test]
    fn action_card_playable_true_for_increase_population() {
        let mut p = blank_player(0, card("Despotism"));
        p.yellow_bank = 5; // prices the increase at 5
        p.food = 5;
        let state = one_player_state(p);
        assert!(action_card_playable(&state, &state.players[0], card("Frugality (A)")));
    }

    #[test]
    fn action_card_playable_true_for_build_one_wonder_stage() {
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids");
        p.resources = 1; // first stage costs 3, Engineering Genius's discount is 2
        let state = one_player_state(p);
        assert!(action_card_playable(&state, &state.players[0], card("Engineering Genius (A)")));
    }

    #[test]
    fn action_card_playable_true_for_develop_technology() {
        let mut p = blank_player(0, card("Despotism"));
        p.hand_civil.push(card("Bronze")); // scienceCost 0: always developable
        let state = one_player_state(p);
        assert!(action_card_playable(&state, &state.players[0], card("Breakthrough (I)")));
    }

    #[test]
    fn action_card_playable_true_for_upgrade_farm_mine_or_urban_building() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert!(action_card_playable(&state, &state.players[0], card("Efficient Upgrade (II)")));
    }

    // -------------------------------------------------------- free_action_moves

    #[test]
    fn free_action_increase_population_ignores_the_discount() {
        let mut p = blank_player(0, card("Despotism"));
        p.yellow_bank = 20; // cost 2
        p.food = 2;
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::IncreasePopulation, 100, false);
        assert!(out.as_slice().contains(&Move::Pop));
    }

    #[test]
    fn free_action_build_one_wonder_stage() {
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids");
        p.resources = 100;
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::BuildOneWonderStage, 0, false);
        assert!(out.as_slice().contains(&Move::WonderStep { steps: 1 }));
    }

    #[test]
    fn free_action_build_one_wonder_stage_applies_the_discount() {
        let mut p = blank_player(0, card("Despotism"));
        p.wonder = card("Pyramids"); // first stage costs 3
        p.resources = 2; // short of 3, but covered by a discount of 1
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::BuildOneWonderStage, 1, false);
        assert!(out.as_slice().contains(&Move::WonderStep { steps: 1 }));

        let state2 = one_player_state({
            let mut p2 = blank_player(0, card("Despotism"));
            p2.wonder = card("Pyramids");
            p2.resources = 2;
            p2
        });
        let out2 = free_action_moves(&state2, &state2.players[0], FreeActionKind::BuildOneWonderStage, 0, false);
        assert!(out2.is_empty(), "no discount: 2 resources is not enough for a cost-3 stage");
    }

    #[test]
    fn free_action_build_one_wonder_stage_needs_a_wonder_in_progress() {
        let p = blank_player(0, card("Despotism"));
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::BuildOneWonderStage, 0, false);
        assert!(out.is_empty());
    }

    #[test]
    fn free_action_develop_technology_from_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.science = 10;
        p.hand_civil.push(card("Irrigation"));
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::DevelopTechnology, 0, false);
        assert!(out.as_slice().contains(&Move::Develop { card: card("Irrigation") }));
    }

    #[test]
    fn free_action_build_or_upgrade_farm_or_mine() {
        let mut p = blank_player(0, card("Despotism"));
        p.workers_free = 1;
        p.resources = 10;
        p.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::BuildOrUpgradeFarmOrMine, 0, false);
        assert!(out.as_slice().contains(&Move::Build { card: card("Bronze") }));
    }

    #[test]
    fn free_action_upgrade_only_kind_never_builds() {
        let mut p = blank_player(0, card("Despotism"));
        p.workers_free = 1;
        p.resources = 10;
        p.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::UpgradeFarmMineOrUrbanBuilding, 0, false);
        assert!(!out.as_slice().iter().any(|m| matches!(m, Move::Build { .. })), "upgrade_only kinds never build");
    }

    #[test]
    fn free_action_upgrade_farm_mine_or_urban_building() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 10;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        let out = free_action_moves(&state, &state.players[0], FreeActionKind::UpgradeFarmMineOrUrbanBuilding, 0, false);
        assert!(out.as_slice().contains(&Move::Upgrade { from: card("Agriculture"), to: card("Irrigation") }));
    }

    // ----------------------------------------------------------- can_revolt

    #[test]
    fn can_revolt_false_for_despotism_which_prints_no_revolution_cost() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 999;
        let state = one_player_state(p);
        assert!(!can_revolt(&state, &state.players[0], card("Despotism")), "revolutionCost: null");
    }

    #[test]
    fn can_revolt_needs_enough_science_and_every_civil_action_unspent() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4; // ca_total(Despotism) == 4
        p.science = 2; // Monarchy's revolutionCost
        let state = one_player_state(p);
        assert!(can_revolt(&state, &state.players[0], card("Monarchy")));

        let mut under = blank_player(0, card("Despotism"));
        under.civil_actions = 4;
        under.science = 1; // one short
        let state_under = one_player_state(under);
        assert!(!can_revolt(&state_under, &state_under.players[0], card("Monarchy")));

        let mut spent = blank_player(0, card("Despotism"));
        spent.civil_actions = 3; // already spent one this turn
        spent.science = 2;
        let state_spent = one_player_state(spent);
        assert!(!can_revolt(&state_spent, &state_spent.players[0], card("Monarchy")));
    }

    #[test]
    fn can_revolt_robespierre_pays_with_military_actions_instead() {
        // Despotism's base 2 + Robespierre's own +1 = 3 total military
        // actions (`Special::RevolutionUsesMilitaryActionsInstead`).
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Maximilien Robespierre");
        p.military_actions = 3; // all unspent
        p.science = 2;
        let state = one_player_state(p);
        assert!(can_revolt(&state, &state.players[0], card("Monarchy")));

        let mut spent = blank_player(0, card("Despotism"));
        spent.leader = card("Maximilien Robespierre");
        spent.military_actions = 2; // one already spent
        spent.science = 2;
        let state_spent = one_player_state(spent);
        assert!(!can_revolt(&state_spent, &state_spent.players[0], card("Monarchy")));
    }
}
