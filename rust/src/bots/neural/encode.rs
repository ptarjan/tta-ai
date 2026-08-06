//! Torch-free tensor encoding of a [`GameState`] from one player's viewpoint.
//!
//! Ports `engine/bots/neural_encode.py` (419 lines) -- read that file's own
//! module doc comment first (reproduced only in part below); it is the
//! design rationale -- what is legal to encode and why (docs/
//! INFORMATION_AUDIT.md's card-counting ruling), which fields are public vs
//! private per seat, and the deliberate omissions (deck ORDER, a rival's
//! military-hand CONTENTS, other players' event seeds) -- and is not
//! restated here.
//!
//! ## Reused, not restated
//!
//! * [`super::super::weighted::features::sweep_tableau`] -- the tableau
//!   worker/tech-level/best-type sweep. Python has this loop TWICE
//!   (`weighted.py::features()` and `neural_encode.py::_player_block`, never
//!   shared); this port shares it. See `sweep_tableau`'s own doc comment for
//!   the one behavioural difference between the two Python call sites (the
//!   government's level is added by the CALLER, not the shared sweep) and
//!   why it is preserved here exactly.
//! * [`crate::costs::row_cost`] -- the civil-action cost by row slot
//!   (`engine/actions.py::ROW_COST`), rather than a second copy of the
//!   `(1,1,1,1,1,2,2,2,2,3,3,3,3)` literal Python's own comment says it
//!   duplicates on purpose to stay import-cycle-free.
//! * [`crate::combat::pacts_for`] -- every pact `idx` is a party to, exactly
//!   the "which pacts is idx a party to" answer `weighted::features` already
//!   reuses this for. Python's `sum(1 for q in state.players for pc in
//!   q.pacts if idx in (pc["owner"], pc["partner"]))` is the identical
//!   question phrased as a double loop over a dict-shaped pact.
//! * [`crate::economy`]/[`crate::effects::compute`] -- happiness/population/
//!   corruption and the per-player `Stats` recomputation, exactly as
//!   `weighted::features` uses them.
//! * [`super::super::weighted::horizon::{rounds_left, lateness, live_count}`]
//!   -- the same functions `neural_encode.py` imports from `weighted.py`.
//!   Python wraps the call in `try/except Exception: rl, lat = 20.0, 0.0` for
//!   a state so degenerate `rounds_left`/`lateness` might raise; both
//!   functions are total here (already exercised across every fixture state
//!   by `weighted_horizon.rs`), so there is no exception path to guard.
//!
//! ## No `lru_cache`
//!
//! Python's `card_vec(name)` is `@lru_cache(maxsize=None)` because building
//! the tuple costs a dict lookup plus per-key dict access, worth memoizing
//! against a small, immutable card database. [`card_vec_into`]/
//! [`card_vec_array`] below read `CardId::get()`'s already-static `&'static
//! Card` (an O(1) array index, `cards.rs`'s own top doc comment) and write a
//! fixed-size stack array -- no allocation, no hashing, nothing a cache would
//! save. A cache here would be a lazily-populated global purely to mirror a
//! CPython performance detail this port never has, which is exactly the
//! Pythonism this project's house style rules out.
//!
//! ## The card-identity vector: typed fields, not a stringly-keyed dict
//!
//! Python's `PROD_KEYS`/`EFF_KEYS` read a card's raw JSON `production`/
//! `effects` dicts by string key. This port's card data is already typed
//! (`cards::Production`/`cards::CardEffects` struct fields, `cards.rs`'s own
//! top doc comment: "a card is a `CardId`, which is an index... names exist
//! at the I/O boundary and nowhere else"), so [`card_vec_array`] reads named
//! struct fields directly -- there is no string-keyed lookup to give the
//! `weight_key_table!`-style enum treatment to in the first place. What DOES
//! need that treatment is the one-hot TYPE slot, whose position is an
//! arbitrary, Python-defined ordering unrelated to [`CardType`]'s own
//! declaration order: [`card_type_slot`] is an exhaustive `match`, one arm
//! per [`CardType`] variant, so a card type this project adds later without
//! a matching arm here is a compile error rather than a silently-absent
//! column.
//!
//! ## Six `EFF_KEYS` columns are permanently dead, verified against the data
//!
//! Python's ORIGINAL `EFF_KEYS` (before the 2026-08-05 fixes below) read a
//! card's TOP-LEVEL `effects` dict only -- not `immediateEffects`, not
//! `permanentEffects`, not a pact's nested `A`/`B`/`bothPlayers` sub-blocks.
//! Checked against every card in `data/*.json` (2026-08-05): `food`,
//! `resources`, `cultureProduction`, `scienceProduction`, `foodProduction`
//! and `resourceProduction` NEVER appear at that top level on any of the 236
//! base-game cards -- the four `*Production` keys are pact-block-only
//! (nested under `A`/`B`), and no card at all prints a bare `food`/
//! `resources` numeric effect anywhere this port's card data reads from
//! (`CardEffects::food`/`resources` stay `0` on every card in
//! `card_table.rs` today). [`card_vec_array`] leaves those six columns at
//! their `0.0` default -- not a bug, just currently always zero because no
//! printed card populates them, kept for vector-shape parity with Python and
//! because a future card could someday print one.
//!
//! `civilActions`/`militaryActions`/`culture`/`science`/`happy`/`strength`/
//! `yellowTokens`/`blueTokens` DO carry real, nonzero values on today's card
//! data, but THREE of those eight needed a fix (2026-08-05, both engines) to
//! read the RIGHT source location:
//!
//! * `civilActions`/`militaryActions` are bare TOP-LEVEL card fields on the 8
//!   base-game governments (`engine/effects.py:420-421` reads
//!   `gov.get("civilActions")` directly), never inside the generic `effects`
//!   dict -- so Python's `card_vec` silently read `0.0` for every
//!   government's civil/military action grant until fixed. This port never
//!   had the bug: `Card.effects.civil_actions`/`military_actions` are
//!   already populated correctly for governments by `gen_cards.py`.
//! * `yellowTokens`/`blueTokens` (for colonization territory cards, §11.5)
//!   and `strength` (ditto, one card: "Strategic Territory") live in
//!   `permanentEffects`, not `effects`, for those cards -- Python's
//!   `card_vec` silently read `0.0` for a territory's permanent token/
//!   strength grant until fixed. Again, this port never had the bug: those
//!   three fields on [`crate::cards::CardEffects`] are already merged
//!   correctly.
//!
//! See `tests::government_civil_military_grant_is_encoded_not_silently_zero`
//! and `tests::territory_permanent_effects_are_encoded_not_silently_zero`.
//!
//! ## `strength` is split across TWO columns by card type, not one shared read
//!
//! Python's card data has `strength` in two DIFFERENT, mutually-exclusive
//! JSON locations, and `card_vec` reads each into a different output column:
//!
//! * nested `effects.strength` OR (territory cards only, after the
//!   2026-08-05 fix above) `permanentEffects.strength` (EFF_KEYS index 7) --
//!   a PERMANENT strength bonus while the card is in play. Checked against
//!   the data (2026-08-05): printed on exactly 9 `effects`-nested cards (2
//!   wonders, 1 leader, 6 special-techs) plus 2 `permanentEffects`-nested
//!   printings (one territory, both ages), and NEVER on a unit or tactic.
//! * bare top-level `strength` (the trailing slot) -- a unit or tactic's
//!   printed combat rating. Printed on exactly 25 cards and ALWAYS a unit
//!   (`Infantry`/`Cavalry`/`Artillery`/`Air`) or a `Tactic`, never anything
//!   else.
//!
//! `cards.rs`'s own doc comment on [`crate::cards::CardEffects::strength`]
//! records that `gen_cards.py` folds BOTH of these into that single field as
//! "the single source" -- deliberately, to avoid the same fact living in two
//! registries. That is the right call for gameplay (nothing in `effects.rs`/
//! `combat.rs` needs to tell the two apart), but it means THIS module, which
//! must reproduce Python's two-column split, cannot read the JSON location
//! back out of `CardEffects` -- so it reconstructs it from `card.kind()`
//! instead, which the data above shows is an exact (not approximate)
//! predicate for which of the two locations a real value came from:
//! `kind.is_unit() || kind == Tactic` selects the trailing slot and zeros
//! EFF_KEYS index 7; every other kind does the reverse. A card that were
//! ever both (impossible in the data checked, and not a shape `card_table.rs`
//! generates) would silently double-count, which is what keeps this a
//! documented data assumption rather than a structural guarantee.

use crate::cards::{Age, CardId, CardType};
use crate::combat;
use crate::costs;
use crate::economy;
use crate::effects;
use crate::state::{GameState, MAX_PLAYERS, ROW_SIZE};

use super::super::weighted::features::sweep_tableau;
use super::super::weighted::horizon;

// ---------------------------------------------------------------------------
// card identity vector
// ---------------------------------------------------------------------------

/// The 23 base-game card types, in the ORDER Python's `CARD_TYPES` tuple
/// lists them (unrelated to [`CardType`]'s own declaration order) --
/// [`card_type_slot`]'s exhaustive `match` is the only way to reach a slot,
/// so a card type added without an arm here fails to compile.
pub const NUM_CARD_TYPES: usize = 23;

#[inline]
fn card_type_slot(kind: CardType) -> usize {
    match kind {
        CardType::Farm => 0,
        CardType::Mine => 1,
        CardType::Lab => 2,
        CardType::Temple => 3,
        CardType::Library => 4,
        CardType::Arena => 5,
        CardType::Theater => 6,
        CardType::Infantry => 7,
        CardType::Cavalry => 8,
        CardType::Artillery => 9,
        CardType::Air => 10,
        CardType::Government => 11,
        CardType::SpecialTech => 12,
        CardType::Wonder => 13,
        CardType::Leader => 14,
        CardType::Tactic => 15,
        CardType::Aggression => 16,
        CardType::War => 17,
        CardType::Pact => 18,
        CardType::Bonus => 19,
        CardType::Territory => 20,
        CardType::Event => 21,
        CardType::Action => 22,
    }
}

/// `PROD_KEYS` count: culture, science, food, resources, happy, strength.
const PROD_KEYS: usize = 6;
/// `EFF_KEYS` count -- see this module's top doc comment for which of the
/// 15 are permanently zero on the current card database and why.
const EFF_KEYS: usize = 15;

/// Type one-hot + level + prod + eff + `[techCost, buildCost, stagesSum,
/// strength]` -- Python's `CARD_VEC_DIM`.
pub const CARD_VEC_DIM: usize = NUM_CARD_TYPES + 1 + PROD_KEYS + EFF_KEYS + 4;

/// A fixed-length identity/effect vector for one card, zero for
/// [`CardId::NONE`] or -- Python's "unknown" branch has no counterpart here,
/// this crate's card table is closed by construction (`cards.rs` rule 1).
///
/// Returned on the stack (`CARD_VEC_DIM` is 49 -- ~400 bytes), not heap --
/// see this module's top doc comment on why no cache is needed either.
fn card_vec_array(id: CardId) -> [f64; CARD_VEC_DIM] {
    let mut v = [0.0f64; CARD_VEC_DIM];
    if id.is_none() {
        return v;
    }
    let card = id.get();

    v[card_type_slot(card.kind)] = 1.0;
    let mut o = NUM_CARD_TYPES;
    v[o] = id.level() as f64 / 4.0;
    o += 1;

    // PROD_KEYS: culture, science, food, resources, happy, strength.
    let prod = card.production;
    v[o] = prod.culture as f64 / 4.0;
    v[o + 1] = prod.science as f64 / 4.0;
    v[o + 2] = prod.food as f64 / 4.0;
    v[o + 3] = prod.resources as f64 / 4.0;
    v[o + 4] = prod.happy as f64 / 4.0;
    v[o + 5] = prod.strength as f64 / 4.0;
    o += PROD_KEYS;

    // EFF_KEYS: civilActions, militaryActions, culture, science, food(dead),
    // resources(dead), happy, strength, cultureProduction(dead),
    // scienceProduction(dead), foodProduction(dead), resourceProduction(dead),
    // yellowTokens, blueTokens, drawMilitaryCards(dead). See this module's
    // top doc comment for the still-dead columns' verification and for why
    // `strength` (index 7) is gated by card kind rather than read
    // unconditionally -- a unit/tactic's combat rating lives in the OTHER
    // "strength" column (the trailing slot below), never here.
    //
    // `yellowTokens`/`blueTokens` read [`crate::cards::CardEffects`]'s
    // already-MERGED fields, which fold in a colonization territory's
    // `permanentEffects.yellowTokens`/`blueTokens` (§11.5) alongside the
    // ordinary top-level `effects` source every other card uses -- there is
    // only one concept here ("this card's permanent token grant while in
    // play"), unlike `strength`'s two genuinely different meanings, so no
    // gating is needed: the merged value is correct for every card kind.
    let is_unit_or_tactic = card.kind.is_unit() || card.kind == CardType::Tactic;
    let eff = card.effects;
    v[o] = eff.civil_actions as f64 / 4.0;
    v[o + 1] = eff.military_actions as f64 / 4.0;
    v[o + 2] = eff.culture as f64 / 4.0;
    v[o + 3] = eff.science as f64 / 4.0;
    v[o + 6] = eff.happy as f64 / 4.0;
    if !is_unit_or_tactic {
        v[o + 7] = eff.strength as f64 / 4.0;
    }
    v[o + 12] = eff.yellow_tokens as f64 / 4.0;
    v[o + 13] = eff.blue_tokens as f64 / 4.0;
    o += EFF_KEYS;

    // techCost, buildCost, stagesSum, and a unit/tactic-only top-level
    // strength (mutually exclusive with EFF_KEYS index 7 above -- see this
    // module's top doc comment).
    v[o] = card.science_cost as f64 / 8.0;
    v[o + 1] = card.resource_cost as f64 / 8.0;
    let stages_sum: u32 = card.stages.iter().map(|&s| s as u32).sum();
    v[o + 2] = stages_sum as f64 / 8.0;
    v[o + 3] = if is_unit_or_tactic { eff.strength as f64 / 6.0 } else { 0.0 };

    v
}

/// Append one card's identity vector to `out`. House style: no fresh `Vec`
/// allocated in a small helper -- the temporary is a stack array
/// ([`card_vec_array`]), and only the final `extend_from_slice` touches the
/// caller's `Vec`.
fn push_card_vec(id: CardId, out: &mut Vec<f64>) {
    out.extend_from_slice(&card_vec_array(id));
}

/// Element-wise sum of [`card_vec_array`] over a hand, appended to `out`.
/// Zero-vector for an empty hand, matching Python's `_sum_card_vec`.
fn push_sum_card_vec(ids: &[CardId], out: &mut Vec<f64>) {
    let mut acc = [0.0f64; CARD_VEC_DIM];
    for &id in ids {
        let v = card_vec_array(id);
        for i in 0..CARD_VEC_DIM {
            acc[i] += v[i];
        }
    }
    out.extend_from_slice(&acc);
}

// ---------------------------------------------------------------------------
// one player's block
// ---------------------------------------------------------------------------

/// Length of the per-player scalar block (`_PLAYER_SCALARS` in Python) --
/// asserted by `tests::player_scalars_len_matches_the_encoded_block`.
const PLAYER_SCALARS: usize = 56;

/// `_PLAYER_SCALARS + 5 * CARD_VEC_DIM`: government/leader/wonder identity
/// vectors plus the summed civil- and military-hand vectors.
pub const PLAYER_BLOCK_DIM: usize = PLAYER_SCALARS + 5 * CARD_VEC_DIM;

/// Append player `idx`'s block to `out`. See Python's `_player_block` for the
/// full field-by-field rationale (docs/INFORMATION_AUDIT.md); this is a
/// direct arithmetic port, not restated here.
///
/// `full = (idx == me_idx)` gates the two fields private to a seat: the
/// military-hand CONTENTS and the seeded-event summary are real only for me;
/// a rival's block zeros both so every seat's block is the same length.
fn player_block(state: &GameState, idx: u8, me_idx: u8, best_rival_strength: i32, out: &mut Vec<f64>) {
    let full = idx == me_idx;
    let p = &state.players[idx as usize];
    let s = effects::compute(state, p);

    // See `sweep_tableau`'s own doc comment: the government's level is NOT
    // added here, unlike `weighted::features` -- `neural_encode.py::
    // _player_block` never adds it either.
    let sweep = sweep_tableau(p);

    let happy_req = economy::happy_required(p.yellow_bank);
    let margin = (s.happy - happy_req as i32) as f64;
    let discontent = (-margin).max(0.0);
    let blue = economy::blue_available(p);
    let pop_cost = economy::pop_food_cost(s.pop_food_discount, p.yellow_bank, 0).unwrap_or(8);

    let mut progress = 0i32;
    let mut remaining = 0i32;
    if !p.wonder.is_none() {
        let stages = p.wonder.get().stages;
        let built = (p.wonder_steps as usize).min(stages.len());
        progress = stages[..built].iter().map(|&st| i32::from(st)).sum();
        remaining = stages[built..].iter().map(|&st| i32::from(st)).sum();
    }

    let rel = f64::from(s.strength) - f64::from(best_rival_strength);

    let mut seeded_n = 0u32;
    let mut seeded_lv = 0u32;
    if full {
        for (cid, &owner) in state.seeded_by.iter().enumerate() {
            if owner == idx {
                seeded_n += 1;
                seeded_lv += CardId(cid as u16).level() as u32;
            }
        }
    }

    let pacts = combat::pacts_for(state, idx).count() as f64;

    let tactic_level = if !p.tactic.is_none() { p.tactic.level() as f64 / 4.0 } else { 0.0 };
    let government_level = if !p.government.is_none() { p.government.level() as f64 / 4.0 } else { 0.0 };

    let scal: [f64; PLAYER_SCALARS] = [
        1.0, // present
        f64::from(p.culture) / 100.0,
        f64::from(p.science) / 50.0,
        f64::from(p.food) / 50.0,
        f64::from(p.resources) / 50.0,
        f64::from(s.culture) / 30.0,
        f64::from(s.science) / 30.0,
        f64::from(s.food) / 20.0,
        f64::from(s.resources) / 20.0,
        f64::from(s.strength) / 30.0,
        f64::from(s.happy) / 10.0,
        f64::from(s.civil_actions) / 8.0,
        f64::from(s.military_actions) / 6.0,
        f64::from(p.yellow_bank) / 18.0,
        f64::from(p.workers_free) / 10.0,
        f64::from(blue) / 16.0,
        f64::from(economy::corruption(blue)) / 5.0,
        f64::from(economy::consumption(p.yellow_bank)) / 10.0,
        f64::from(pop_cost) / 8.0,
        margin.min(3.0) / 3.0,
        discontent / 10.0,
        if discontent > f64::from(p.workers_free) { 1.0 } else { 0.0 },
        f64::from(p.civil_actions) / 8.0,
        f64::from(p.military_actions) / 6.0,
        f64::from(p.ca_spent_taking) / 6.0,
        f64::from(sweep.best.farm) / 4.0,
        f64::from(sweep.best.mine) / 4.0,
        f64::from(sweep.best.lab) / 4.0,
        f64::from(sweep.best.temple) / 4.0,
        f64::from(sweep.best.theater) / 4.0,
        f64::from(sweep.best.library) / 4.0,
        f64::from(sweep.best.arena) / 4.0,
        f64::from(sweep.best_unit) / 4.0,
        f64::from(sweep.prod_workers) / 15.0,
        f64::from(sweep.urban_workers) / 15.0,
        f64::from(sweep.unit_workers) / 15.0,
        f64::from(sweep.workers) / 15.0,
        p.techs.len() as f64 / 25.0,
        f64::from(sweep.special_techs) / 12.0,
        f64::from(sweep.tech_levels) / 60.0,
        p.completed_wonders.len() as f64 / 8.0,
        f64::from(progress) / 12.0,
        f64::from(remaining) / 12.0,
        if !p.wonder.is_none() { 1.0 } else { 0.0 },
        tactic_level,
        p.colonies.len() as f64 / 3.0,
        pacts / 5.0,
        rel / 30.0,
        (-rel).max(0.0) / 30.0,
        rel.max(0.0).min(6.0) / 6.0,
        p.hand_size_civil() as f64 / 12.0,
        p.hand_size_military() as f64 / 12.0,
        f64::from(seeded_n) / 10.0,
        f64::from(seeded_lv) / 20.0,
        government_level,
        if !p.leader.is_none() { 1.0 } else { 0.0 },
    ];
    out.extend_from_slice(&scal);

    // card-identity blocks (each CARD_VEC_DIM long)
    push_card_vec(p.government, out);
    push_card_vec(p.leader, out);
    push_card_vec(p.wonder, out);
    push_sum_card_vec(p.hand_civil.as_slice(), out); // public for everyone
    // military hand contents: mine only; rival gets a zero-vector (count is
    // already in the scalar block above via `hand_size_military`).
    if full {
        push_sum_card_vec(p.hand_military.as_slice(), out);
    } else {
        out.extend_from_slice(&[0.0; CARD_VEC_DIM]);
    }
}

fn push_empty_player_block(out: &mut Vec<f64>) {
    out.extend(std::iter::repeat(0.0).take(PLAYER_BLOCK_DIM));
}

// ---------------------------------------------------------------------------
// global block
// ---------------------------------------------------------------------------

/// Every [`Age`] has five variants (A=0..IV=4), and its discriminant IS
/// Python's `C.AGES.index(age)` -- both orderings are "starting age first,
/// then I/II/III/IV" -- so the one-hot is a plain array write, no lookup.
fn push_onehot_age(age: Age, out: &mut Vec<f64>) {
    let mut v = [0.0f64; 5];
    v[age as usize] = 1.0;
    out.extend_from_slice(&v);
}

const GLOBAL_DIM: usize = 3 * 5 + 3 + 9 + 3 + 2 * 5;

fn push_global_block(state: &GameState, out: &mut Vec<f64>) {
    let n = state.num_players;
    push_onehot_age(state.age_civil, out);
    push_onehot_age(state.age_military, out);
    push_onehot_age(state.current_events_age, out);

    let mut npc = [0.0f64; 3];
    if (2..=4).contains(&n) {
        npc[(n - 2) as usize] = 1.0;
    }
    out.extend_from_slice(&npc);

    let live = horizon::live_count(state);
    let rl = horizon::rounds_left(state, live);
    let lat = horizon::lateness(state);
    let row_occ = state.card_row.iter().filter(|&&c| !c.is_none()).count();

    out.extend_from_slice(&[
        f64::from(state.round) / 30.0,
        f64::from(state.turn) / 60.0,
        rl / 30.0,
        lat,
        if state.last_round { 1.0 } else { 0.0 },
        if !state.scoring_events.is_empty() { 1.0 } else { 0.0 },
        state.current_events.len() as f64 / 10.0,
        state.future_events.len() as f64 / 10.0,
        row_occ as f64 / 13.0,
    ]);

    use crate::state::Phase;
    out.extend_from_slice(&[
        if state.phase == Phase::Politics { 1.0 } else { 0.0 },
        if state.phase == Phase::Actions { 1.0 } else { 0.0 },
        if state.phase == Phase::Done { 1.0 } else { 0.0 },
    ]);

    push_discard_block(state, out);
}

/// Divisors for [`push_discard_block`] -- see Python's own extensive comment
/// on `_CIVIL_DISCARD_SCALE`/`_MIL_DISCARD_SCALE` for why these are flat
/// constants rather than "share of that age's own deck": reading the deck
/// size would mean calling into the card-count machinery on a path the
/// search runs once per node, for a scale factor `num_players` and both ages
/// (already encoded next door) let a net learn on its own.
const CIVIL_DISCARD_SCALE: f64 = 30.0;
const MIL_DISCARD_SCALE: f64 = 20.0;

/// Both discard piles, per age -- the card-counting primitive. See Python's
/// own extensive comment on `_discard_block` for why this MUST be the union
/// of `civil_discard` and `civil_removed` (every card out of play, not just
/// the ones swept off the row) and why a per-age SIZE, not per-card identity,
/// is what is exposed.
fn push_discard_block(state: &GameState, out: &mut Vec<f64>) {
    let mut civil = [0.0f64; 5];
    let mut mil = [0.0f64; 5];
    for age in 0..5 {
        let civil_n = state.civil_discard[age].len() + state.civil_removed[age].len();
        civil[age] = (civil_n as f64 / CIVIL_DISCARD_SCALE).min(1.0);
        mil[age] = (state.discarded_military[age].len() as f64 / MIL_DISCARD_SCALE).min(1.0);
    }
    out.extend_from_slice(&civil);
    out.extend_from_slice(&mil);
}

// ---------------------------------------------------------------------------
// the card row
// ---------------------------------------------------------------------------

const ROW_DIM: usize = ROW_SIZE * (2 + CARD_VEC_DIM);

fn push_row_block(state: &GameState, out: &mut Vec<f64>) {
    for i in 0..ROW_SIZE {
        let id = state.card_row[i];
        out.push(if !id.is_none() { 1.0 } else { 0.0 });
        out.push(costs::row_cost(i) as f64 / 3.0);
        push_card_vec(id, out);
    }
}

// ---------------------------------------------------------------------------
// top level
// ---------------------------------------------------------------------------

/// Flat encoding length for player `idx` -- Python's `ENCODING_DIM`/
/// `encoding_dim()`.
pub const ENCODING_DIM: usize = GLOBAL_DIM + ROW_DIM + MAX_PLAYERS * PLAYER_BLOCK_DIM;

/// Flat `Vec<f64>` of length [`ENCODING_DIM`] for player `idx`, from that
/// player's viewpoint -- Python's `encode(state, idx)`. One allocation (the
/// return value itself), matching every other top-level encoder/evaluator in
/// this crate (`weighted::features::features`, `weighted::eval::evaluate`);
/// every helper this function calls writes into it rather than allocating
/// its own.
pub fn encode(state: &GameState, idx: u8) -> Vec<f64> {
    let mut out = Vec::with_capacity(ENCODING_DIM);
    push_global_block(state, &mut out);
    push_row_block(state, &mut out);

    let n = state.num_players;
    let live_players = &state.players[..n as usize];

    // Best rival strength for `idx` itself (my relative-strength features),
    // computed once.
    let mut best_rs = 0i32;
    for q in live_players {
        if q.idx != idx && !q.resigned {
            best_rs = best_rs.max(effects::compute(state, q).strength);
        }
    }
    player_block(state, idx, idx, best_rs, &mut out);

    // Rivals in seating order after me, up to MAX_PLAYERS-1, zero-padded.
    let rival_count = (n - 1).min((MAX_PLAYERS - 1) as u8);
    for k in 1..=rival_count {
        let r = (idx + k) % n;
        // A rival's relative strength is measured against ITS best rival.
        let mut best = 0i32;
        for q in live_players {
            if q.idx != r && !q.resigned {
                best = best.max(effects::compute(state, q).strength);
            }
        }
        player_block(state, r, idx, best, &mut out);
    }
    for _ in 0..(MAX_PLAYERS as u8 - 1 - rival_count) {
        push_empty_player_block(&mut out);
    }

    debug_assert_eq!(out.len(), ENCODING_DIM, "encode() produced the wrong length");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game as G;

    /// Every [`CardType`] variant maps to a distinct slot in `0..NUM_CARD_TYPES`
    /// -- if `card_type_slot` ever collided, this would be the first thing to
    /// notice, well before a differential test caught it.
    #[test]
    fn card_type_slot_is_injective_and_in_range() {
        let all = [
            CardType::Farm,
            CardType::Mine,
            CardType::Lab,
            CardType::Temple,
            CardType::Library,
            CardType::Arena,
            CardType::Theater,
            CardType::Infantry,
            CardType::Cavalry,
            CardType::Artillery,
            CardType::Air,
            CardType::Government,
            CardType::SpecialTech,
            CardType::Wonder,
            CardType::Leader,
            CardType::Action,
            CardType::Tactic,
            CardType::Aggression,
            CardType::War,
            CardType::Pact,
            CardType::Bonus,
            CardType::Territory,
            CardType::Event,
        ];
        let mut seen = [false; NUM_CARD_TYPES];
        for k in all {
            let slot = card_type_slot(k);
            assert!(slot < NUM_CARD_TYPES, "{k:?}: slot {slot} out of range");
            assert!(!seen[slot], "{k:?}: slot {slot} collides with an earlier type");
            seen[slot] = true;
        }
        assert!(seen.iter().all(|&b| b), "not every slot in 0..NUM_CARD_TYPES was used");
    }

    /// [`card_vec_array`] on [`CardId::NONE`] is the all-zero vector.
    #[test]
    fn card_vec_of_none_is_zero() {
        assert_eq!(card_vec_array(CardId::NONE), [0.0; CARD_VEC_DIM]);
    }

    /// Regression for the 2026-08-05 Python fix (`engine/bots/
    /// neural_encode.py::card_vec`, `tests/test_neural_encode.py::
    /// test_government_civil_military_grant_is_encoded`): a government
    /// prints its civil/military action grant as a TOP-LEVEL card field
    /// (`engine/effects.py:420-421` reads `gov.get("civilActions")`
    /// directly), never inside the generic `effects` dict every other
    /// card's grant lives in -- Python's `card_vec` used to read a silent
    /// `0.0` for every one of the 8 base-game governments because it only
    /// ever looked in the nested dict. This port never had the bug (`Card.
    /// effects.civil_actions`/`military_actions` are already populated
    /// correctly for governments by `gen_cards.py`), so this pins the
    /// CORRECT value directly rather than a before/after diff -- the "before"
    /// state only ever existed in Python.
    #[test]
    fn government_civil_military_grant_is_encoded_not_silently_zero() {
        let despotism = CardId::by_name("Despotism").unwrap();
        let v = card_vec_array(despotism);
        let eff_start = NUM_CARD_TYPES + 1 + PROD_KEYS;
        assert_eq!(v[eff_start], 4.0 / 4.0, "civilActions must not read as the silent-zero bug");
        assert_eq!(v[eff_start + 1], 2.0 / 4.0, "militaryActions must not read as the silent-zero bug");
    }

    /// Regression for the 2026-08-05 Python fix (same shape as the
    /// government one above): a colonization territory (§11.5) prints its
    /// permanent yellow/blue-token and strength grant in `permanentEffects`,
    /// never in the generic `effects` dict. `Card::effects` on this port
    /// already merges `permanentEffects` in at generation time, so this
    /// module reads it correctly by construction -- this test pins that a
    /// territory's grant reaches the vector at all, not the silent zero
    /// Python's `card_vec` used to produce.
    #[test]
    fn territory_permanent_effects_are_encoded_not_silently_zero() {
        let vast = CardId::by_name("Vast Territory (I)").unwrap();
        let v = card_vec_array(vast);
        let eff_start = NUM_CARD_TYPES + 1 + PROD_KEYS;
        assert_ne!(v[eff_start + 12], 0.0, "yellowTokens must not read as the silent-zero bug");
        assert_ne!(v[eff_start + 13], 0.0, "blueTokens must not read as the silent-zero bug");

        let strategic = CardId::by_name("Strategic Territory (I)").unwrap();
        let sv = card_vec_array(strategic);
        assert_ne!(sv[eff_start + 7], 0.0, "a territory's permanentEffects.strength must not read as zero");
    }

    /// A real card's one-hot type slot is set and nothing else in that block
    /// is -- a coarse sanity check that `card_type_slot` and the offset
    /// arithmetic in `card_vec_array` actually line up.
    #[test]
    fn card_vec_sets_exactly_one_type_slot() {
        let id = CardId::by_name("Warriors").unwrap();
        let v = card_vec_array(id);
        let ones: Vec<usize> = (0..NUM_CARD_TYPES).filter(|&i| v[i] == 1.0).collect();
        assert_eq!(ones, vec![card_type_slot(CardType::Infantry)]);
    }

    /// [`push_sum_card_vec`] of an empty hand is the zero vector, and of a
    /// two-card hand is the element-wise sum of each card's own vector.
    #[test]
    fn sum_card_vec_is_the_elementwise_sum() {
        let mut empty = Vec::new();
        push_sum_card_vec(&[], &mut empty);
        assert_eq!(empty, vec![0.0; CARD_VEC_DIM]);

        let a = CardId::by_name("Warriors").unwrap();
        let b = CardId::by_name("Irrigation").unwrap();
        let mut summed = Vec::new();
        push_sum_card_vec(&[a, b], &mut summed);
        let va = card_vec_array(a);
        let vb = card_vec_array(b);
        for i in 0..CARD_VEC_DIM {
            assert_eq!(summed[i], va[i] + vb[i], "coordinate {i}");
        }
    }

    /// The per-player scalar block is exactly [`PLAYER_SCALARS`] long --
    /// pins the block-length arithmetic against silent drift if a field is
    /// added to `player_block`'s `scal` array without updating the constant.
    #[test]
    fn player_scalars_len_matches_the_encoded_block() {
        let state = G::new_game(2, 1);
        let mut out = Vec::new();
        player_block(&state, 0, 0, 0, &mut out);
        assert_eq!(out.len(), PLAYER_BLOCK_DIM);
    }

    /// `encode()` always returns exactly [`ENCODING_DIM`] floats, for every
    /// player count and every seat.
    #[test]
    fn encode_len_is_always_encoding_dim() {
        for n in [2u8, 3, 4] {
            let state = G::new_game(n, 7);
            for idx in 0..n {
                let v = encode(&state, idx);
                assert_eq!(v.len(), ENCODING_DIM, "{n}p idx {idx}");
            }
        }
    }

    /// Rival slots beyond the real player count are the all-zero empty
    /// block, and the boundary between the last real rival and the first
    /// padding slot is exactly at `PLAYER_BLOCK_DIM`.
    #[test]
    fn encode_zero_pads_missing_rivals_in_a_2p_game() {
        let state = G::new_game(2, 8);
        let v = encode(&state, 0);
        let players_start = GLOBAL_DIM + ROW_DIM;
        // slot 0: me: not all zero (at least the "present" scalar is 1.0).
        let me = &v[players_start..players_start + PLAYER_BLOCK_DIM];
        assert_eq!(me[0], 1.0, "the 'present' scalar must be 1.0 for a real seat");
        // slot 1: the one real rival in a 2p game.
        let rival = &v[players_start + PLAYER_BLOCK_DIM..players_start + 2 * PLAYER_BLOCK_DIM];
        assert_eq!(rival[0], 1.0);
        // slots 2, 3: padding -- MAX_PLAYERS - 1 - 1 = 2 empty blocks.
        for pad in 2..4 {
            let block = &v[players_start + pad * PLAYER_BLOCK_DIM..players_start + (pad + 1) * PLAYER_BLOCK_DIM];
            assert!(block.iter().all(|&x| x == 0.0), "padding block {pad} must be all-zero");
        }
    }

    /// The military-hand identity vector is real for me and a zero-vector
    /// for a rival, even though both hold the same card -- the private-hand
    /// gate in `player_block`'s `full` branch.
    #[test]
    fn rival_military_hand_contents_are_hidden_but_mine_are_not() {
        let mut state = G::new_game(2, 9);
        let phalanx = CardId::by_name("Phalanx").unwrap();
        state.players[0].hand_military.push(phalanx);
        state.players[1].hand_military.push(phalanx);

        let v0 = encode(&state, 0);
        let players_start = GLOBAL_DIM + ROW_DIM;
        // Player 0's own block (me): military-hand vector is the last
        // CARD_VEC_DIM of the block.
        let my_block = &v0[players_start..players_start + PLAYER_BLOCK_DIM];
        let my_mil_hand = &my_block[PLAYER_BLOCK_DIM - CARD_VEC_DIM..];
        assert_ne!(my_mil_hand, card_vec_array(CardId::NONE).as_slice(), "my own military hand must be visible");

        // Player 1's block, from player 0's viewpoint (rival slot 1): the
        // military-hand vector must be all zero even though player 1 holds
        // the identical card.
        let rival_block = &v0[players_start + PLAYER_BLOCK_DIM..players_start + 2 * PLAYER_BLOCK_DIM];
        let rival_mil_hand = &rival_block[PLAYER_BLOCK_DIM - CARD_VEC_DIM..];
        assert_eq!(rival_mil_hand, &[0.0; CARD_VEC_DIM], "a rival's military hand contents must be hidden");
        // ...but its SIZE is still visible, through the scalar block (index
        // 51: hand_size_military / 12.0).
        assert!(rival_block[51] > 0.0, "a rival's military hand SIZE must still be visible");
    }

    /// My own seeded-event summary is nonzero when I have seeded one; a
    /// rival's identical seed is invisible from my own viewpoint (I only
    /// know what I planted, not what a rival did).
    #[test]
    fn only_my_own_seeded_events_are_summarised() {
        let mut state = G::new_game(3, 10);
        let event = crate::cards::CARDS
            .iter()
            .enumerate()
            .find(|(_, c)| c.kind == CardType::Event)
            .map(|(i, _)| CardId(i as u16))
            .unwrap();
        state.seeded_by[event.0 as usize] = 1; // player 1 seeded it

        let v_seer = encode(&state, 1); // from the seeder's own viewpoint
        let v_other = encode(&state, 0); // from a different player's viewpoint

        let players_start = GLOBAL_DIM + ROW_DIM;
        // index 52/53 in the scalar block: seeded_n / seeded_lv.
        let me_block = &v_seer[players_start..players_start + PLAYER_BLOCK_DIM];
        assert!(me_block[52] > 0.0, "the seeder must see its own seeded_n as nonzero");

        let other_me_block = &v_other[players_start..players_start + PLAYER_BLOCK_DIM];
        assert_eq!(other_me_block[52], 0.0, "a different player must not see player 1's seed as its own");
    }

    /// `encode()` never mutates the state it reads.
    #[test]
    fn encode_never_mutates_the_real_state() {
        let state = G::new_game(3, 11);
        let before = state.clone();
        let _ = encode(&state, 0);
        assert_eq!(state.card_row, before.card_row);
        assert_eq!(state.players[0].culture, before.players[0].culture);
    }

    /// Pinned against `python3.13 -c "from engine.bots import neural_encode as
    /// NE; print(NE.encoding_dim())"` (1907, checked 2026-08-05) -- the
    /// (since-retired) differential test `rust/tests/neural.rs` used to
    /// check every coordinate on real states, but this is the one-line guard
    /// that fires first and fastest if the two ever disagree about the
    /// vector's LENGTH.
    #[test]
    fn encoding_dim_matches_python() {
        assert_eq!(ENCODING_DIM, 1907, "must match engine.bots.neural_encode.encoding_dim()");
    }
}
