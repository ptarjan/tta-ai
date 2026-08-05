//! Population/happiness/corruption tables and the blue-token bank.
//!
//! Ports `engine/economy.py` (§6 of `docs/RULES_SPEC.md`). All numbers below
//! follow that spec exactly; where this file's shape differs from the
//! Python's it is because DESIGN.md rule "the Python source is the spec for
//! the rules, never the representation" licenses it, and every such place is
//! called out below.
//!
//! ## What is NOT here, and why
//!
//! `economy.py` also holds `pop_cost`, `discontent`, `uprising`,
//! `_end_of_turn_leader_bonus` and the `end_of_turn` orchestrator. All of
//! them read `effects.state_stats(state, p)` for at least one of
//! `happy` / `science` / `culture` / `civil_actions` / `military_actions` /
//! `strength` / `pop_food_discount`. `effects.rs` does not exist in this
//! crate yet (it is a separate worker's module, and this port must not touch
//! or pre-empt it), so none of those five functions can be written against a
//! real `Stats` type today. What CAN be ported without it:
//!
//! - [`pop_food_cost`]: Python already splits this out as "the pure formula
//!   that takes the discount as a value" versus `pop_cost`, "the state-
//!   reading wrapper" (see the Python docstring) -- so the pure half ports
//!   cleanly, and the wrapper is a one-line call once `effects::state_stats`
//!   exists: `pop_food_cost(stats.pop_food_discount, p.yellow_bank,
//!   one_time_food_discount)`.
//! - [`increase_population`]: ported as "move the token, given an
//!   already-known `cost`" -- the same split, for the same reason. The
//!   caller computes `cost` from `pop_food_cost` once it can.
//!
//! `discontent`, `uprising` and `_end_of_turn_leader_bonus` are each a
//! one-line combination of a table function here with a `Stats` field, so
//! there is nothing to port ahead of `effects.rs` landing; they are not
//! stubbed here to avoid a second copy of "the formula" that could drift.
//!
//! `end_of_turn` itself needs, beyond `Stats`: (a) `interact.rs`'s decision
//! queue for step 1 (discard down to the military hand limit) -- `GameState`
//! has no `pending`-decision field yet, so this is a `state.rs` gap, not
//! only a missing module; and (b) a Python-compatible seeded shuffle for
//! step 4 (see [`draw_military`]). Both are cross-cutting infrastructure
//! this module should not invent unilaterally.
//!
//! `discard_military` and `discard_civil` (deck-departure bookkeeping) ARE
//! self-contained and are ported below.

use crate::cards::{CardId, CardType};
use crate::state::{GameState, PlayerState, Tableau};

// --------------------------------------------------------------- tables

/// Food to increase population (§6.1). `None` when the bank is empty.
pub fn pop_cost_base(yellow_bank: u8) -> Option<u8> {
    if yellow_bank == 0 {
        return None;
    }
    Some(if yellow_bank >= 17 {
        2
    } else if yellow_bank >= 13 {
        3
    } else if yellow_bank >= 9 {
        4
    } else if yellow_bank >= 5 {
        5
    } else {
        7
    })
}

/// Food eaten in the production phase (§6.1).
pub fn consumption(yellow_bank: u8) -> u8 {
    if yellow_bank >= 17 {
        0
    } else if yellow_bank >= 13 {
        1
    } else if yellow_bank >= 9 {
        2
    } else if yellow_bank >= 5 {
        3
    } else if yellow_bank >= 1 {
        4
    } else {
        6
    }
}

/// Happy faces needed to keep everyone content (§6.1/§6.3).
pub fn happy_required(yellow_bank: u8) -> u8 {
    if yellow_bank >= 17 {
        0
    } else if yellow_bank >= 13 {
        1
    } else if yellow_bank >= 11 {
        2
    } else if yellow_bank >= 9 {
        3
    } else if yellow_bank >= 7 {
        4
    } else if yellow_bank >= 5 {
        5
    } else if yellow_bank >= 3 {
        6
    } else if yellow_bank >= 1 {
        7
    } else {
        8
    }
}

/// Resources lost each production phase (§6.2), keyed by how many blue
/// tokens are free (not occupied by stored food/resources/wonder steps).
pub fn corruption(blue_available: u16) -> u16 {
    if blue_available >= 11 {
        0
    } else if blue_available >= 6 {
        2
    } else if blue_available >= 1 {
        4
    } else {
        6
    }
}

// ------------------------------------------------------- derived checks

/// Food to increase population, given an already-known pop-food discount
/// (§6.1). THE single implementation of the formula -- see the module docs
/// above for why `pop_cost`, the `&GameState`/`&PlayerState`-taking wrapper,
/// is not ported yet.
///
/// `one_time_food_discount` mirrors Python's
/// `(one_time.get("increasePopulation") or {}).get("food", 0)`; pass `0`
/// when there is no pending one-time discount. Both discounts can exceed
/// `pop_cost_base` (a one-shot "free population increase" grant, or a large
/// `pop_food_discount` stack), which is why the intermediate subtraction is
/// signed and only the FINAL result is floored at zero -- exactly what the
/// Python does by subtracting `one_time` first and `max(0, ...)`-ing once at
/// the end, not after each subtraction.
pub fn pop_food_cost(
    pop_food_discount: i32,
    yellow_bank: u8,
    one_time_food_discount: i32,
) -> Option<i32> {
    let base = pop_cost_base(yellow_bank)? as i32;
    let base = base - one_time_food_discount;
    Some((base - pop_food_discount).max(0))
}

// ---------------------------------------------------- population helpers

/// Move a token from the yellow bank into the worker pool (§3.3), given the
/// already-computed food cost (`0` for a free increase, e.g. a card grant).
///
/// See the module docs: the `pop_cost(state, p)` lookup Python does inline
/// needs `effects::state_stats`, which this crate does not have yet, so the
/// caller computes `cost` (via [`pop_food_cost`], once `effects.rs` lands)
/// and passes it in -- the same split Python draws between the pure formula
/// and the state-reading wrapper.
///
/// Python also calls `effects.invalidate(state, p)` on success. There is no
/// equivalent here: that call exists solely to dirty Python's memoized
/// `state_stats` cache (`effects.py`'s `_STATS_CACHE_KEY`), a performance
/// hack for an expensive dict-walking `compute()`. This port's `compute` is
/// meant to be a fast, uncached field-sum over `Tableau` (DESIGN.md: "the
/// cost IS the dynamic lookups"), so there is nothing to invalidate.
pub fn increase_population(p: &mut PlayerState, cost: u16) -> bool {
    if p.yellow_bank == 0 {
        return false;
    }
    if p.food < cost {
        return false;
    }
    p.food -= cost;
    p.yellow_bank -= 1;
    p.workers_free += 1;
    true
}

/// "Lose 1 population": an unused worker first, else one off a card (§6.x).
///
/// Which card gets weakened is decided by tableau order: Python iterates
/// `p.techs`, a `dict`, so it walks build order and takes the worker off the
/// first worker-holding card it finds. That is arbitrary as a rule but not as
/// a position -- losing a farm worker is not losing a mine worker.
///
/// This was flagged during the port as a divergence risk, because `Tableau`
/// used a swap-remove and so lost build order the first time any card left
/// play. `Tableau::remove` is now order-preserving (see the note on it in
/// state.rs), which makes `Tableau::iter()` the right order here. Do not
/// reintroduce a swap-remove without dealing with this function and with
/// `legal_moves`.
pub fn lose_population(p: &mut PlayerState) -> bool {
    if p.workers_free > 0 {
        p.workers_free -= 1;
        p.yellow_bank += 1;
        return true;
    }
    let mut target: Option<CardId> = None;
    for (id, slot) in p.techs.iter() {
        if slot.workers > 0 && id.kind().takes_workers() {
            target = Some(id);
            break;
        }
    }
    match target {
        Some(id) => {
            p.techs.get_mut(id).expect("id came from this tableau").workers -= 1;
            p.yellow_bank += 1;
            true
        }
        None => false,
    }
}

// ------------------------------------------------------- blue token math
//
// §6.4: a blue token on a farm/mine card is worth that card's printed
// food/resource value. The number of tokens a scalar `food`/`resources`
// total occupies is derived, not stored, by a minimal-token greedy over the
// denominations of the farm/mine cards in play -- that derivation is what
// keeps `blue_available` (and therefore `corruption`) faithful to the
// physical token bank the rulebook describes. `Card::production` in the
// static table carries that denomination: a farm's is `production.food`, a
// mine's is `production.resources` (cards.rs `Production`: "food/resources
// double as the blue-token denomination for §6.4").

/// Denominations of blue tokens available to a player for cards of type
/// `kind` (`Farm` or `Mine`) -- mirrors Python `effects._denoms`. Sorted
/// descending, deduplicated, always includes `1` (the bank's plain token,
/// present even with no farm/mine techs in play). Fixed capacity: the base
/// game has four farm techs and four mine techs today (denominations
/// 1/2/3/5 each), so eight slots is generous headroom without a `Vec`
/// (DESIGN.md rule 3).
struct Denoms {
    values: [u8; 8],
    len: u8,
}

impl Denoms {
    /// `kind` selects both the card type to scan and which side of
    /// [`Production`](crate::cards::Production) is the denomination: a farm's
    /// blue-token value is its printed `food`, a mine's is its `resources` --
    /// the two fields a card never prints both of at once (cards.rs).
    fn of(techs: &Tableau, kind: CardType) -> Self {
        let mut d = Denoms { values: [0; 8], len: 0 };
        d.push(1);
        for (id, _) in techs.of_type(kind) {
            let prod = id.get().production;
            let v = match kind {
                CardType::Farm => prod.food,
                CardType::Mine => prod.resources,
                _ => 0,
            };
            if v > 0 && v <= u8::MAX as i16 {
                d.push(v as u8);
            }
        }
        d.values[..d.len as usize].sort_unstable_by(|a, b| b.cmp(a));
        d
    }

    fn push(&mut self, v: u8) {
        if !self.values[..self.len as usize].contains(&v) {
            debug_assert!((self.len as usize) < self.values.len(), "Denoms overflow");
            self.values[self.len as usize] = v;
            self.len += 1;
        }
    }

    fn as_slice(&self) -> &[u8] {
        &self.values[..self.len as usize]
    }
}

/// Minimal number of blue tokens holding `amount`, greedy over `denoms`
/// (§6.4; 1 is always a present denomination in practice). NOT monotonic in
/// `amount`: e.g. with denominations `[3, 1]`, `amount=2` costs 2 tokens
/// (two 1s) but `amount=3` costs only 1 (one 3) -- gaining food can free up
/// tokens by consolidating into a higher card. [`gain_food`]/
/// [`gain_resources`] rely on exactly this non-monotonicity (see there).
pub fn tokens_for(mut amount: u16, denoms: &[u8]) -> u16 {
    let mut n: u16 = 0;
    for &d in denoms {
        if d == 0 {
            continue;
        }
        let d = d as u16;
        while amount >= d {
            amount -= d;
            n += 1;
        }
    }
    // Python: `n + max(0, amount)`. `amount` is unsigned here (never went
    // negative -- the loop only subtracts while `amount >= d`), so the
    // `max(0, ...)` is a no-op; kept as a plain add rather than ported
    // 1:1, since there is no sign to floor.
    n + amount
}

/// Blue tokens currently occupied: stored food, stored resources, and any
/// wonder-construction steps already paid for (§6.4; a wonder step is
/// worth one blue token, same as any other card slot).
pub fn blue_used(p: &PlayerState) -> u16 {
    let food_denoms = Denoms::of(&p.techs, CardType::Farm);
    let mine_denoms = Denoms::of(&p.techs, CardType::Mine);
    let mut used =
        tokens_for(p.food, food_denoms.as_slice()) + tokens_for(p.resources, mine_denoms.as_slice());
    if !p.wonder.is_none() {
        used += p.wonder_steps as u16;
    }
    used
}

/// Blue tokens still in the bank, free to be spent gaining food/resources
/// or paying corruption (§6.4).
pub fn blue_available(p: &PlayerState) -> u16 {
    (p.blue_total as u16).saturating_sub(blue_used(p))
}

/// Shared body of `gain_food`/`gain_resources` (Python `effects._gain`):
/// searches DOWNWARD from `n` for the largest amount that can be gained
/// without the token cost exceeding what is free in the bank. Downward,
/// not upward, because [`tokens_for`] is not monotonic (see there) -- the
/// marginal cost of gaining `want` can be NEGATIVE (a consolidation), so
/// the search must start at the player's actual desired amount and only
/// give up on it in favour of a smaller one, never the reverse.
fn gain(cur: u16, n: u16, free: u16, denoms: &[u8]) -> u16 {
    if n == 0 || free == 0 {
        return 0;
    }
    let base = tokens_for(cur, denoms) as i32;
    let mut want = n;
    while want > 0 {
        let delta = tokens_for(cur + want, denoms) as i32 - base;
        if delta <= free as i32 {
            return want;
        }
        want -= 1;
    }
    0
}

/// Gain up to `n` food, limited by the blue bank (§6.4). Returns the amount
/// actually gained (may be less than `n`, or `0` with an empty bank).
pub fn gain_food(p: &mut PlayerState, n: u16) -> u16 {
    let denoms = Denoms::of(&p.techs, CardType::Farm);
    let got = gain(p.food, n, blue_available(p), denoms.as_slice());
    p.food += got;
    got
}

/// Gain up to `n` resources, limited by the blue bank (§6.4).
pub fn gain_resources(p: &mut PlayerState, n: u16) -> u16 {
    let denoms = Denoms::of(&p.techs, CardType::Mine);
    let got = gain(p.resources, n, blue_available(p), denoms.as_slice());
    p.resources += got;
    got
}

/// Pay `n` resources. Food covering any shortfall is NOT done here -- the
/// caller (§6.6 step 3b in `end_of_turn`) falls back to food itself.
/// Returns the amount actually paid (may be less than `n`).
pub fn pay_resources(p: &mut PlayerState, n: u16) -> u16 {
    let paid = p.resources.min(n);
    p.resources -= paid;
    paid
}

// ---------------------------------------------------- military deck I/O

/// Record a military card leaving play into the age-keyed discard pile.
///
/// Python's `_DB.age_of(name) if name in _DB.by_name else state.age_military`
/// exists to handle a `name` that is not a real card (defensive, for
/// whatever Python string happened to be passed); a `CardId` here is always
/// either a real card or [`CardId::NONE`], so the fallback only fires for
/// `NONE`. There is no `journal.touch` equivalent: Python's journal is an
/// undo-log for `GreedyBot`'s trial moves (`engine/journal.py`) that exists
/// only because a Python dict copy is cheaper than a full state copy; this
/// port's `GameState` is `Clone`-as-memcpy by design (DESIGN.md rule 3), so
/// trial moves just clone the state and there is nothing to journal.
pub fn discard_military(state: &mut GameState, card: CardId) {
    let age = if card.is_none() { state.age_military } else { card.get().age };
    state.discarded_military[age as usize].push(card);
}

/// Record a civil card leaving play into `civil_removed` (see `state.rs`:
/// this is provenance, NOT `civil_discard`, which means "swept off the row"
/// to the neural encoder and must not be widened). Same fallback and same
/// journal note as [`discard_military`].
pub fn discard_civil(state: &mut GameState, card: CardId) {
    let age = if card.is_none() { state.age_civil } else { card.get().age };
    state.civil_removed[age as usize].push(card);
}

// `draw_military` (§6.6 step 4) is deliberately NOT ported. Its fast path
// (deck non-empty: pop one card) would be a one-liner, but the reshuffle
// path needs two things this module cannot supply on its own:
//
//   1. `CardList<N>` (state.rs) exposes no mutable-slice accessor, so there
//      is no way to shuffle the reclaimed discard pile in place without
//      either copying it out through `as_slice()` into a local array (which
//      still needs somewhere to write the shuffled order back to) or adding
//      a method to `CardList` -- and this port is scoped to
//      `economy.rs` only, not `state.rs`. Flagging this prominently: ANY
//      future in-place shuffle (colonization pools, event/tactic decks) hits
//      the identical wall.
//   2. Python seeds a fresh `random.Random(state.seed * 7919 + state.turn)`
//      per call and shuffles with CPython's Mersenne-Twister-backed
//      `Random.shuffle`. Matching that bit-for-bit for the differential
//      harness needs a from-scratch MT19937 + Fisher-Yates port shared by
//      every RNG-consuming module (colonization bidding, event/tactic
//      shuffles, bots) -- a cross-cutting piece of infrastructure, not a
//      one-off worth inventing inside `economy.rs`. The empty `Cargo.toml`
//      also deliberately forbids reaching for the `rand` crate without
//      justifying a new dependency (see `Cargo.toml`'s own comment).
//
// Whoever adds the shared RNG module and/or a `CardList` mutable accessor
// should add `draw_military` here afterward; the rest of `end_of_turn`'s
// step 4 (`for _ in 0..min(3, max(0, p.military_actions)) { ... }`) is a
// trivial loop once that exists.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{CardList, PactList, Phase, TechSlot, MAX_HAND, MAX_PLAYERS};

    // ---- test scaffolding: PlayerState/GameState derive no Default, so a
    // full-field literal lives here once and every test builds off it.

    fn blank_player(idx: u8) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government: CardId::NONE,
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
            hand_civil: CardList::<MAX_HAND>::new(),
            hand_military: CardList::<MAX_HAND>::new(),
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

    fn blank_state() -> GameState {
        GameState {
            num_players: 2,
            seed: 0,
            players: [blank_player(0), blank_player(1), blank_player(2), blank_player(3)],
            current: 0,
            turn: 1,
            round: 1,
            start_player: 0,
            age_civil: crate::cards::Age::A,
            age_military: crate::cards::Age::A,
            civil_deck: CardList::new(),
            military_deck: CardList::new(),
            card_row: [CardId::NONE; crate::state::ROW_SIZE],
            future_events: CardList::new(),
            current_events: CardList::new(),
            past_events: CardList::new(),
            current_events_age: crate::cards::Age::A,
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

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    // ------------------------------------------------------------ tables
    // Oracle values are `engine.economy`'s own values (also asserted by
    // Python's `tests/test_engine.py::TestEconomyTables`), reread directly
    // off `engine/economy.py` for this port.

    #[test]
    fn pop_cost_base_bands() {
        let expect: &[(u8, u8)] =
            &[(18, 2), (17, 2), (16, 3), (13, 3), (12, 4), (9, 4), (8, 5), (5, 5), (4, 7), (1, 7)];
        for &(bank, cost) in expect {
            assert_eq!(pop_cost_base(bank), Some(cost), "bank={bank}");
        }
        assert_eq!(pop_cost_base(0), None);
    }

    #[test]
    fn consumption_bands() {
        let expect: &[(u8, u8)] =
            &[(18, 0), (17, 0), (16, 1), (13, 1), (12, 2), (9, 2), (8, 3), (5, 3), (4, 4), (1, 4), (0, 6)];
        for &(bank, c) in expect {
            assert_eq!(consumption(bank), c, "bank={bank}");
        }
    }

    #[test]
    fn happy_required_bands() {
        let expect: &[(u8, u8)] = &[
            (18, 0),
            (17, 0),
            (16, 1),
            (13, 1),
            (12, 2),
            (11, 2),
            (10, 3),
            (9, 3),
            (8, 4),
            (7, 4),
            (6, 5),
            (5, 5),
            (4, 6),
            (3, 6),
            (2, 7),
            (1, 7),
            (0, 8),
        ];
        for &(bank, h) in expect {
            assert_eq!(happy_required(bank), h, "bank={bank}");
        }
    }

    #[test]
    fn corruption_bands() {
        let expect: &[(u16, u16)] = &[(16, 0), (11, 0), (10, 2), (6, 2), (5, 4), (1, 4), (0, 6)];
        for &(blue, corr) in expect {
            assert_eq!(corruption(blue), corr, "blue={blue}");
        }
    }

    // --------------------------------------------------------- pop_food_cost

    #[test]
    fn pop_food_cost_matches_base_when_undiscounted() {
        assert_eq!(pop_food_cost(0, 14, 0), Some(3));
        assert_eq!(pop_food_cost(0, 0, 0), None);
    }

    #[test]
    fn pop_food_cost_applies_both_discounts_and_floors_at_zero() {
        // base(14) = 3; a 2-food one-time discount then a 5 pop_food_discount
        // would go negative -- Python floors ONCE, at the end, not per step.
        assert_eq!(pop_food_cost(5, 14, 2), Some(0));
        // discount smaller than the total: 3 - 1 (one-time) - 1 (stat) = 1
        assert_eq!(pop_food_cost(1, 14, 1), Some(1));
    }

    // -------------------------------------------------------- lose_population

    #[test]
    fn lose_population_prefers_an_unused_worker() {
        let mut p = blank_player(0);
        p.workers_free = 2;
        p.yellow_bank = 0;
        assert!(lose_population(&mut p));
        assert_eq!(p.workers_free, 1);
        assert_eq!(p.yellow_bank, 1);
    }

    #[test]
    fn lose_population_falls_back_to_a_card() {
        let mut p = blank_player(0);
        p.workers_free = 0;
        p.yellow_bank = 0;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        assert!(lose_population(&mut p));
        assert_eq!(p.techs.workers(card("Agriculture")), 0);
        assert_eq!(p.yellow_bank, 1);
    }

    #[test]
    fn lose_population_fails_with_nothing_to_take() {
        let mut p = blank_player(0);
        assert!(!lose_population(&mut p));
    }

    // ------------------------------------------------------ increase_population

    #[test]
    fn increase_population_pays_cost_and_moves_a_token() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 10;
        assert!(increase_population(&mut p, 4));
        assert_eq!(p.food, 6);
        assert_eq!(p.yellow_bank, 4);
        assert_eq!(p.workers_free, 1);
    }

    #[test]
    fn increase_population_rejects_short_food_or_empty_bank() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 2;
        assert!(!increase_population(&mut p, 4));
        assert_eq!(p.food, 2, "a rejected attempt must not spend anything");

        p.yellow_bank = 0;
        p.food = 100;
        assert!(!increase_population(&mut p, 0));
    }

    // --------------------------------------------------------- discard_*

    #[test]
    fn discard_military_files_by_the_cards_own_age() {
        let mut st = blank_state();
        st.age_military = crate::cards::Age::II;
        // "Warriors" is Age A; must file under ITS age, not the state's.
        let warriors = card("Warriors");
        discard_military(&mut st, warriors);
        assert_eq!(st.discarded_military[crate::cards::Age::A as usize].as_slice(), &[warriors]);
        assert!(st.discarded_military[crate::cards::Age::II as usize].as_slice().is_empty());
    }

    #[test]
    fn discard_civil_none_falls_back_to_state_age() {
        let mut st = blank_state();
        st.age_civil = crate::cards::Age::III;
        discard_civil(&mut st, CardId::NONE);
        assert_eq!(
            st.civil_removed[crate::cards::Age::III as usize].as_slice(),
            &[CardId::NONE]
        );
    }

    // ------------------------------------------------------ blue token math
    // Oracle values were generated by calling the REAL `engine.effects`
    // functions (`tokens_for`, `_denoms`, `gain_food`, `pay_resources`)
    // against a live `game.new_game` player, not hand-derived -- see the
    // porting session's transcript. `_denoms`/`gain_food` need a card DB
    // lookup Python has and this test recreates with real `CardId`s.

    #[test]
    fn tokens_for_greedy_matches_python_oracle() {
        assert_eq!(tokens_for(0, &[1]), 0);
        assert_eq!(tokens_for(1, &[1]), 1);
        assert_eq!(tokens_for(5, &[1]), 5);
        assert_eq!(tokens_for(7, &[5, 3, 2, 1]), 2); // 5+2
        assert_eq!(tokens_for(11, &[5, 3, 2, 1]), 3); // 5+5+1
        assert_eq!(tokens_for(4, &[3, 1]), 2); // 3+1
        assert_eq!(tokens_for(2, &[3, 1]), 2); // 1+1
        assert_eq!(tokens_for(10, &[5, 2, 1]), 2); // 5+5
        assert_eq!(tokens_for(6, &[5, 2, 1]), 2); // 5+1... wait: 5,then1 -> 2 tokens
        assert_eq!(tokens_for(0, &[5, 3, 2, 1]), 0);
        assert_eq!(tokens_for(16, &[5, 3, 2, 1]), 4); // 5+5+5+1
        assert_eq!(tokens_for(17, &[5, 3, 2, 1]), 4); // 5+5+5+2
    }

    #[test]
    fn denoms_include_one_and_every_farm_denomination_in_play() {
        let mut techs = Tableau::new();
        techs.insert(card("Agriculture"), TechSlot { workers: 0, stored: 0 }); // 1
        techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 }); // 2
        let d = Denoms::of(&techs, CardType::Farm);
        assert_eq!(d.as_slice(), &[2, 1]); // descending, deduped (1 appears once)
    }

    #[test]
    fn blue_used_and_available_with_farm_denominations() {
        let mut p = blank_player(0);
        p.techs.insert(card("Agriculture"), TechSlot { workers: 0, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        p.blue_total = 10;
        assert_eq!(blue_used(&p), 0);
        assert_eq!(blue_available(&p), 10);

        let got = gain_food(&mut p, 7);
        assert_eq!(got, 7);
        assert_eq!(p.food, 7);
        assert_eq!(blue_used(&p), 4); // 3x2 + 1x1

        // Exhaust the bank: only 13 more food fits in the remaining 6 tokens
        // (denominations [2,1]) before the 10-token bank runs out.
        let got2 = gain_food(&mut p, 100);
        assert_eq!(got2, 13);
        assert_eq!(p.food, 20);
        assert_eq!(blue_used(&p), 10);
        assert_eq!(blue_available(&p), 0);

        // Bank is empty: nothing more can be gained.
        assert_eq!(gain_food(&mut p, 1), 0);
    }

    #[test]
    fn gain_food_with_no_farm_tech_uses_only_the_base_token() {
        let mut p = blank_player(1);
        p.blue_total = 5;
        assert_eq!(gain_food(&mut p, 3), 3);
        assert_eq!(p.food, 3);
        // 2 tokens left in the bank; asking for 10 caps at 2.
        assert_eq!(gain_food(&mut p, 10), 2);
        assert_eq!(p.food, 5);
    }

    #[test]
    fn gain_food_consolidates_into_a_higher_denomination() {
        // Denominations {3, 1}: 2 food costs 2 tokens (two 1s); gaining 1
        // more consolidates to a single 3-token, so the MARGINAL cost is
        // negative and the gain is allowed even though the bank is nearly
        // exhausted.
        let mut p = blank_player(0);
        p.techs.insert(card("Selective Breeding"), TechSlot { workers: 0, stored: 0 }); // denom 3
        p.blue_total = 10;
        p.food = 2;
        assert_eq!(blue_used(&p), 2);
        let got = gain_food(&mut p, 1);
        assert_eq!(got, 1);
        assert_eq!(p.food, 3);
        assert_eq!(blue_used(&p), 1); // one 3-token replaces two 1-tokens
    }

    #[test]
    fn pay_resources_caps_at_what_is_owned() {
        let mut p = blank_player(0);
        p.resources = 3;
        assert_eq!(pay_resources(&mut p, 5), 3);
        assert_eq!(p.resources, 0);
    }

    #[test]
    fn blue_used_counts_wonder_steps() {
        let mut p = blank_player(0);
        p.blue_total = 10;
        p.wonder = card("Pyramids");
        p.wonder_steps = 3;
        assert_eq!(blue_used(&p), 3);
        assert_eq!(blue_available(&p), 7);
    }
}
