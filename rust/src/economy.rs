//! Population/happiness/corruption tables and the blue-token bank.
//!
//! Ports `engine/economy.py` (§6 of `docs/RULES_SPEC.md`). All numbers below
//! follow that spec exactly; where this file's shape differs from the
//! Python's it is because DESIGN.md rule "the Python source is the spec for
//! the rules, never the representation" licenses it, and every such place is
//! called out below.
//!
//! ## What is here now
//!
//! The whole of `economy.py` is ported: the §6.1/§6.2 tables, the blue-token
//! bank (§6.4), the population helpers (§3.3), the deck-departure bookkeeping
//! and -- since `effects.rs` and `rng.rs` landed -- `pop_cost`, `discontent`,
//! `uprising`, `draw_military`, `_end_of_turn_leader_bonus` and the
//! [`end_of_turn`] orchestrator itself (§6.6).
//!
//! §6.6 step 1 -- "discard down to the military hand limit" -- is the ONLY
//! decision the end-of-turn sequence asks the player to make (RB p.20, quoted
//! in RULES_SPEC §6.6: "Once you have decided which military cards to
//! discard, the rest of your turn is automatic"). Python routes it through
//! `interact.discard_excess_military`/`push_choice`; [`end_of_turn`] here
//! calls [`crate::interact::discard_excess_military`] directly rather than
//! carrying a second copy of the rule. (An earlier revision of this file DID
//! carry a private copy, written before `state.pending`/`interact.rs`
//! existed, which `unimplemented!()`d the genuine-choice case -- that copy is
//! gone now that the real decision queue exists; see `interact.rs`'s own doc
//! comment for why two implementations of one rule was the bug class
//! DESIGN.md exists to close.) [`end_of_turn`] preserves Python's suspend
//! contract exactly: it returns `false` when step 1 opened a real decision,
//! with steps 2-5 NOT run -- the caller, `game::resume_end_turn`, queues the
//! resume and hands off; it returns `true` when the sequence ran to the end.
//!
//! One field-level gap remains, and it is not this module's to close:
//! `PlayerState` has no `one_time_discount` (events are not ported), so
//! [`pop_cost`] passes `0` for the one-time food discount, the same
//! already-documented hole `costs.rs::build_cost_for` and `legal.rs` carry.

use crate::cards::{Age, CardId, CardType, Special};
use crate::effects;
use crate::interact;
use crate::rng::{shuffle_cards, PyRandom};
use crate::state::{CardList, GameState, PlayerState, Tableau, MAX_PLAYERS};

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

/// The widest FINITE corruption band (`6..=10`) is five values across, so a
/// player standing at its top can absorb four more occupied blue tokens
/// before §6.2 charges more. Headroom is capped there for two reasons: no
/// single turn crosses two band edges, and in the unbounded top band an
/// uncapped headroom is just `blue_available - 11`, an affine copy of the
/// already-priced `BlueFree` coordinate.
const CORRUPTION_HEADROOM_CAP: u16 = 4;

/// The same for consumption: the widest finite band (`13..=16`) is four
/// values across (§6.4).
const CONSUMPTION_HEADROOM_CAP: u8 = 3;

/// How many MORE blue tokens may be occupied before corruption gets worse.
///
/// [`corruption`] answers "what am I paying now"; this answers "how close am
/// I to paying more", which is the quantity a strong player actually plans a
/// turn around -- spending down to exactly the band edge and no further.
/// Without it a bot is blind to the cliff until it has already fallen off:
/// storing one resource at 11 free blue costs 2 resources per turn forever
/// after, and nothing in the state vector distinguishes that from storing one
/// at 15.
///
/// Zero when already in the worst band, since there is no edge left to cross.
pub fn corruption_headroom(blue_available: u16) -> u16 {
    let band_floor = if blue_available >= 11 {
        11
    } else if blue_available >= 6 {
        6
    } else if blue_available >= 1 {
        1
    } else {
        return 0;
    };
    (blue_available - band_floor).min(CORRUPTION_HEADROOM_CAP)
}

/// How much MORE population may be taken before food consumption gets worse
/// (§6.4) -- the food-side twin of [`corruption_headroom`], and the reason
/// taking the last cheap population is often worth a whole turn of planning.
pub fn consumption_headroom(yellow_bank: u8) -> u8 {
    let band_floor = if yellow_bank >= 17 {
        17
    } else if yellow_bank >= 13 {
        13
    } else if yellow_bank >= 9 {
        9
    } else if yellow_bank >= 5 {
        5
    } else if yellow_bank >= 1 {
        1
    } else {
        return 0;
    };
    (yellow_bank - band_floor).min(CONSUMPTION_HEADROOM_CAP)
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

/// `engine/economy.py::pop_cost` -- the state-reading wrapper around
/// [`pop_food_cost`]'s pure formula. `None` when the yellow bank is empty.
///
/// The one-time discount is `p.one_time_discount.pop_food`, exactly as
/// Python's `pop_cost` passes the real `p.one_time_discount` dict.
/// `legal.rs` grew a private copy of this wrapper while `economy.rs`
/// predated `effects.rs`; that copy calls [`pop_food_cost`] too, so the
/// FORMULA is still single-sourced -- only the two-line wrapper is
/// duplicated, and `legal.rs` is another worker's file.
pub fn pop_cost(state: &GameState, p: &PlayerState) -> Option<i32> {
    let s = effects::state_stats(state, p);
    pop_food_cost(s.pop_food_discount, p.yellow_bank, p.one_time_discount.pop_food as i32)
}

// ------------------------------------------------- Trade Routes Agreement

/// Trade Routes Agreement, side A's half (§5.9, `bga_throughtheages_
/// material.inc.php`: "Civilization A can use 1 food as 1 resource during
/// its turn"): how many food-to-resource conversions `p` may still make
/// THIS turn. Sums every live pact's grant (`effects::state_stats`'s
/// `food_as_resource`, itself the sum of every pact's `PactBlock::
/// food_as_resource` -- a player party to two such grants, from two
/// different partners, really does get two conversions, per `state.rs`'s
/// own doc comment on the two `*_used_this_turn` counters) and subtracts
/// however many `p` has already spent this turn. Never negative: spending
/// cannot outrun the grant because [`crate::apply`]'s handler is the only
/// writer of the counter and `legal::action_moves` is the only place that
/// may call it, both gated on this being `> 0` first.
pub fn trade_food_as_resource_remaining(state: &GameState, p: &PlayerState) -> i32 {
    let s = effects::state_stats(state, p);
    (s.food_as_resource - p.trade_food_as_resource_used_this_turn as i32).max(0)
}

/// The [`trade_food_as_resource_remaining`] twin for Trade Routes' OTHER
/// direction ("Civilization B can use 1 resource as 1 food during its
/// turn").
pub fn trade_resource_as_food_remaining(state: &GameState, p: &PlayerState) -> i32 {
    let s = effects::state_stats(state, p);
    (s.resource_as_food - p.trade_resource_as_food_used_this_turn as i32).max(0)
}

/// Unhappy workers: how far the player's happiness falls short of what §6.1/
/// §6.3 demands for the population already born. Never negative.
pub fn discontent(state: &GameState, p: &PlayerState) -> i32 {
    let s = effects::state_stats(state, p);
    (happy_required(p.yellow_bank) as i32 - s.happy).max(0)
}

/// §6.3: the discontented outnumber the workers who could be sent to placate
/// them, so the civilization rebels and the production phase is skipped.
///
/// Note the STRICT `>`: discontent exactly equal to the free-worker count is
/// not an uprising -- those workers are the ones being spent on the unhappy.
pub fn uprising(state: &GameState, p: &PlayerState) -> bool {
    discontent(state, p) > p.workers_free as i32
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
///
/// `consume_one_time`: whether this increase was a REAL, non-free one that
/// already had Civil Life's one-shot `pop_food` discount folded into `cost`
/// by the caller's `pop_food_cost` call (see
/// [`OneTimeDiscount`](crate::state::OneTimeDiscount)'s doc comment, fixed
/// 2026-08-05). `true` for every paying caller (`apply.rs::
/// h_pop` when `!free`, `h_barbarossa`, `events.rs::paid_increase_population`);
/// `false` for a free grant (`h_pop_free`, an Ocean Liners increase, which
/// never even computes `pop_cost`) so it cannot consume a discount it never
/// looked at.
pub fn increase_population(p: &mut PlayerState, cost: u16, consume_one_time: bool) -> bool {
    if p.yellow_bank == 0 {
        return false;
    }
    if p.food < cost {
        return false;
    }
    p.food -= cost;
    p.yellow_bank -= 1;
    p.workers_free += 1;
    // Development of Civil Life's grant is ONE mutually-exclusive choice
    // (pop XOR build XOR develop), not three independent discounts -- using
    // this one must exhaust the OTHER two as well, not just `pop_food`. Only
    // when `pop_food` was actually live (nonzero): an ordinary, unrelated
    // population increase (no Civil Life grant ever banked) must not wipe a
    // DIFFERENT, still-unspent build/develop grant this player hasn't used
    // yet. See `OneTimeDiscount`'s own doc comment (`state.rs`).
    if consume_one_time && p.one_time_discount.pop_food != 0 {
        p.one_time_discount.exhaust();
    }
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
    let food_tokens = tokens_for(p.food, food_denoms.as_slice());
    let mine_tokens = tokens_for(p.resources, mine_denoms.as_slice());
    let mut used = food_tokens + mine_tokens;
    if !p.wonder.is_none() {
        used += p.wonder_steps as u16;
    }
    if crate::debugflags::replay_debug_all() {
        eprintln!(
            "DEBUG blue_used: food={} food_denoms={:?} food_tokens={food_tokens} resources={} mine_denoms={:?} mine_tokens={mine_tokens} wonder={} wonder_steps={} total={used}",
            p.food, food_denoms.as_slice(), p.resources, mine_denoms.as_slice(),
            if p.wonder.is_none() { "none" } else { p.wonder.get().name }, p.wonder_steps
        );
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

/// The RNG `draw_military` reshuffles with: `random.Random(state.seed * 7919
/// + state.turn)`, constructed FRESH at every reshuffle (`engine/economy.py::
/// _rng`). It is deliberately not the `rng` argument `game.py` threads into
/// `economy.end_of_turn` -- that argument is never read by the Python
/// `end_of_turn` at all, which is why this port's [`end_of_turn`] takes no
/// rng parameter (see its doc comment).
///
/// `state.seed` is a `u64` and `PyRandom` takes an `i128` -- see `rng.rs`'s
/// doc comment on `PyRandom::new` for why `i128` is permanent headroom for a
/// `u64` seed times a small fixed multiplier (`i64` was tried first and
/// overflowed in production; `game::rng_for`'s doc comment has the numbers).
/// Python's ints are unbounded, so a seed large enough to overflow would draw
/// a DIFFERENT shuffle there than any fixed-width Rust integer could here --
/// that is a silent divergence, so it is asserted rather than wrapped or
/// truncated.
fn deck_rng(state: &GameState) -> PyRandom {
    let seed = i128::from(state.seed)
        .checked_mul(7919)
        .and_then(|s| s.checked_add(state.turn as i128))
        .expect(
            "game seed * 7919 + turn overflows i128; Python's unbounded ints would seed a \
             different MT19937 stream -- widen rng::PyRandom::new rather than wrapping",
        );
    PyRandom::new(seed)
}

/// Draw one military card (§6.6 step 4), reshuffling the current age's
/// discard pile back into the deck when the deck runs dry. `None` when both
/// are empty -- the age's military cards are all in hands or in play.
///
/// The reclaimed pile is moved into `military_deck` in discard order and THEN
/// shuffled, and the pile is emptied, exactly as Python does. Drawing is
/// `pop()` -- off the END of the list, which is what `list.pop()` means, so
/// the shuffled order is consumed back-to-front. Getting that end wrong would
/// still deal "a random card" and still diverge from every fixture.
pub fn draw_military(state: &mut GameState) -> Option<CardId> {
    if state.military_deck.is_empty() {
        let age = state.age_military as usize;
        if state.discarded_military[age].is_empty() {
            return None;
        }
        state.military_deck = CardList::new();
        // `for &c in ...push(c)` rather than a slice assignment: the two
        // `CardList`s have the same capacity but the borrow checker cannot
        // know that, and this runs at most once per age.
        for i in 0..state.discarded_military[age].len() {
            let c = state.discarded_military[age].as_slice()[i];
            state.military_deck.push(c);
        }
        state.discarded_military[age] = CardList::new();
        let mut rng = deck_rng(state);
        shuffle_cards(&mut rng, state.military_deck.as_mut_slice());
    }
    state.military_deck.pop()
}

// ------------------------------------------------- end-of-turn sequence

/// §6.6, in exact order. Mutates `state.players[idx]` in place.
///
/// Returns `false` when step 1 suspended on a discard decision and steps 2-5
/// have NOT run; `true` when the sequence ran to the end. Step 1 itself is
/// [`crate::interact::discard_excess_military`] -- see the module doc
/// comment above for why there is exactly one implementation of the rule.
///
/// The order below is the rules text and is load-bearing in ways that are
/// invisible if you get them wrong:
///
///   1. discard excess military cards (the only decision);
///   2. uprising check -- and on an uprising the ENTIRE production phase is
///      skipped, science and culture included, not merely food/resources;
///   3. production: (a) science + culture + the leader bonus, (b) corruption,
///      (c) food production, (d) food consumption, (e) resource production;
///   4. draw military cards;
///   5. reset actions.
///
/// Three orderings inside step 3 are silent scoring bugs if swapped:
///
///   * **Culture is scored BEFORE the famine penalty.** Step (a) adds this
///     turn's culture; step (d) subtracts 4 per missing food from the same
///     stock. Scoring after the famine would let a player who cannot feed
///     their people keep culture the rules take away (and vice versa: the
///     penalty is floored at zero, so which side of it the income lands on
///     changes the result whenever the penalty exceeds the stock).
///   * **Corruption is paid BEFORE food is produced.** Corruption's size is
///     read off `blue_available`, and gaining food OCCUPIES blue tokens; a
///     player who produces first would be assessed corruption against a
///     fuller bank and pay more. Both directions are wrong in real games.
///   * **Consumption is paid out of the food produced this turn.** (c) then
///     (d), not (d) then (c) -- a civilization eats what it just grew.
///
/// The `Stats` used for science/culture/food/resources is computed ONCE, up
/// front (after step 1), and is NOT recomputed as the steps mutate the
/// player. Step 5 takes a second, fresh reading for the action totals.
///
/// Python's signature takes an `rng` and never reads it (`draw_military`
/// derives its own from `state.seed`/`state.turn` -- see [`deck_rng`]), so
/// there is no rng parameter here.
pub fn end_of_turn(state: &mut GameState, idx: u8) -> bool {
    // Python opens with `effects.invalidate(state, p)`; there is no stats
    // cache in this port (see `increase_population`), so there is nothing to
    // invalidate here or at step 5.

    if crate::debugflags::replay_debug_all() {
        eprintln!("DEBUG end_of_turn ENTRY: idx={idx} round={}", state.round);
    }

    // ---- 1. discard excess military cards -----------------------------
    if interact::discard_excess_military(state, idx) {
        if crate::debugflags::replay_debug_all() {
            eprintln!("DEBUG end_of_turn: idx={idx} stopped at discard_excess_military");
        }
        return false;
    }

    let s = effects::state_stats(state, &state.players[idx as usize]);

    // ---- 2. uprising check --------------------------------------------
    // Python emits a log line here (`state.emit(...)`); `GameState` has no
    // journal/emit sink and the string is not read by anything.
    if crate::debugflags::replay_debug_all() {
        let p = &state.players[idx as usize];
        eprintln!(
            "DEBUG uprising check: idx={idx} yellow_bank={} happy_required={} s.happy={} s.science={} discontent={} workers_free={} uprising={}",
            p.yellow_bank, happy_required(p.yellow_bank), s.happy, s.science, discontent(state, p), p.workers_free, uprising(state, p)
        );
    }
    if !uprising(state, &state.players[idx as usize]) {
        // ---- 3a. score science and culture ----------------------------
        {
            let p = &mut state.players[idx as usize];
            // `Stats::science`/`culture` are clamped at zero by
            // `effects::compute` ("Limits on Ratings"), so these additions
            // are never subtractions and the unsigned stocks are safe.
            p.science += s.science as u16;
            p.culture += s.culture as u16;
        }
        end_of_turn_leader_bonus(state, idx);

        let p = &mut state.players[idx as usize];

        // ---- 3b. corruption -------------------------------------------
        // Resources first; food covers whatever the resources could not.
        // `pay_resources` never pays more than it was asked for, so the
        // shortfall cannot go negative.
        let corr = corruption(blue_available(p));
        if crate::debugflags::replay_debug_all() {
            eprintln!(
                "DEBUG end_of_turn pre-corruption: idx={idx} resources={} food={} blue_total={} blue_used={} corr={} s.resources={} s.food={}",
                p.resources, p.food, p.blue_total, blue_used(p), corr, s.resources, s.food
            );
            if crate::debugflags::replay_debug_techs() {
                for (id, slot) in p.techs.iter() {
                    let card = id.get();
                    eprintln!(
                        "  TECH idx={idx} name={} kind={:?} workers={} production.food={}",
                        card.name, card.kind, slot.workers, card.production.food
                    );
                }
            }
        }
        let paid = pay_resources(p, corr);
        let short = corr - paid;
        if short > 0 {
            p.food = p.food.saturating_sub(short);
        }

        // ---- 3c. food production --------------------------------------
        gain_food(p, s.food as u16);
        if crate::debugflags::replay_debug_all() {
            eprintln!("DEBUG end_of_turn post-production: idx={idx} food={} yellow_bank={}", p.food, p.yellow_bank);
        }

        // ---- 3d. food consumption -------------------------------------
        let need = consumption(p.yellow_bank) as u16;
        if p.food >= need {
            p.food -= need;
        } else {
            let missing = need - p.food;
            p.food = 0;
            p.culture = p.culture.saturating_sub(4 * missing);
        }

        // ---- 3e. resource production ----------------------------------
        gain_resources(p, s.resources as u16);
        if crate::debugflags::replay_debug_all() {
            eprintln!(
                "DEBUG end_of_turn POST: idx={idx} resources={} food={} science={} culture={}",
                p.resources, p.food, p.science, p.culture
            );
        }
    }

    // ---- 4. draw military cards ---------------------------------------
    // Never in age IV (the military deck is exhausted by then) and never on
    // round 1. Python also gates on `state.has_military`, a card-DATABASE
    // completeness flag that is always true for the compiled-in base game --
    // the same non-field `legal.rs::politics_moves` documents.
    //
    // The count is `min(3, max(0, p.military_actions))` read BEFORE step 5
    // resets it: it is what the player had LEFT this turn, not what they will
    // have next turn. Reordering 4 and 5 would hand a player who spent every
    // military action a full refill of cards.
    if state.age_military != Age::IV && state.round > 1 {
        let n = state.players[idx as usize].military_actions.clamp(0, 3);
        if crate::debugflags::replay_debug_all() {
            eprintln!(
                "DEBUG draw_military_step: idx={idx} round={} military_actions_unused={} n_drawn={n}",
                state.round,
                state.players[idx as usize].military_actions,
            );
        }
        for _ in 0..n {
            match draw_military(state) {
                Some(card) => state.players[idx as usize].hand_military.push(card),
                None => break,
            }
        }
    }

    // ---- 5. reset actions ---------------------------------------------
    // A FRESH `Stats`: steps 3 and 4 can have changed what the player has in
    // play only via the leader bonus (which cannot), but Python re-reads here
    // and the re-read is what makes this robust to that changing.
    let s = effects::state_stats(state, &state.players[idx as usize]);
    let p = &mut state.players[idx as usize];
    p.civil_actions = (s.civil_actions - p.ca_penalty_next_turn as i32).max(0) as i8;
    p.ca_penalty_next_turn = 0;
    p.military_actions = s.military_actions as i8;
    p.tactic_action_used = false;
    p.hammurabi_used = false;
    p.hammurabi_replaced_this_turn = false;
    p.replaced_leader_this_turn = false;
    p.trade_food_as_resource_used_this_turn = 0;
    p.trade_resource_as_food_used_this_turn = 0;
    p.churchill_used = false;
    p.bach_upgrade_used = false;
    p.ocean_liners_used = false;
    // Homer's once-per-turn resource (§`costs::homer_unit_discount`'s own
    // doc comment) refreshes for next turn, same lifetime as the other
    // once-per-turn flags immediately above.
    p.homer_used_this_turn = false;
    p.politics_done = false;
    p.caesar_second_politics = false;
    // Backstop for Joan of Arc's look: `apply::end_politics` clears it when
    // the phase closes, and a turn that never had a politics phase never set
    // it, but a stale name here would be a lie about what this seat knows.
    // Mirrors `engine/economy.py::end_of_turn`'s own backstop comment.
    p.peeked_event = CardId::NONE;
    p.taken_this_turn = CardList::new();
    // §3.11: action-card discount pools expire at end of turn.
    if crate::debugflags::replay_debug_all() && p.mil_discount != 0 {
        eprintln!("DEBUG mil_discount site=end_of_turn idx={idx} reset {} -> 0", p.mil_discount);
    }
    p.mil_discount = 0;
    // Churchill's ring-fenced science, same lifetime.
    p.mil_sci_discount = 0;
    true
}

/// §6.6 step 3a tail: Genghis Khan scores 3 culture per turn while his owner
/// is at or above SECOND place in military strength.
///
/// `strengths` includes the Khan's own strength, so the general (3+ player)
/// test is "my strength is >= the second-highest strength in the game", i.e.
/// at most one rival out-arms me. A tie at second place still scores (`>=`).
/// With fewer than two civilizations still in the game there is no second
/// place and the bonus is unconditional.
///
/// A prior version of this function treated exactly-two-players as
/// "vacuously true" for that same "top two" test -- mathematically correct
/// in isolation (`strengths[1]` is `min(mine, theirs)`, so `mine >=
/// strengths[1]` always holds), but the WRONG rule: the CoL rulebook's own
/// appendix names Genghis Khan specifically, not just the general "N of M
/// civilizations" clause: "In a two-player game, 'one of the two strongest'
/// should be read as 'the strongest'. (You still win ties.)" -- i.e. the
/// THRESHOLD collapses from top-2 to top-1 in a 2p game, it does not
/// disappear. Traced against real game `7522205` (2p, `docs/REPLAY.md`'s
/// culture-oracle TakeCard bucket): round 8, BGO's own "Genghis Khan scores
/// 0 culture" line (Orange was NOT the stronger of the two civilizations
/// that turn) against the old code's unconditional +3 -- an ENGINE bug, not
/// a replayer artifact, since it would misscore any 2p game against a human
/// the same way live.
///
/// Python dispatches on the leader's NAME (`if p.leader == "Genghis Khan"`)
/// and hardcodes the 3. This reads `Special::CultureIfTopTwoStrength(n)` off
/// the leader card instead: same card, but the magnitude comes from the card
/// table rather than from a string compare against a literal (DESIGN.md rule
/// 5 / "an unhandled case is a compile error, not a silent skip"). Genghis is
/// the only carrier of that variant today, asserted by
/// `tests::only_genghis_khan_carries_the_top_two_strength_special` so a
/// second carrier cannot appear without this function noticing.
///
/// Only the LEADER slot is scanned, matching Python exactly -- a wonder or
/// tech carrying the same special would score nothing there either, which is
/// what the assertion above exists to catch.
fn end_of_turn_leader_bonus(state: &mut GameState, idx: u8) {
    let p = &state.players[idx as usize];
    if p.leader.is_none() {
        return;
    }
    let mut bonus = 0i32;
    for &sp in p.leader.get().special {
        if let Special::CultureIfTopTwoStrength(n) = sp {
            bonus += n as i32;
        }
    }
    if bonus == 0 {
        return;
    }
    let mine = effects::state_stats(state, p).strength;
    let mut strengths = [0i32; MAX_PLAYERS];
    let mut n = 0usize;
    for q in state.active() {
        strengths[n] = effects::state_stats(state, q).strength;
        n += 1;
    }
    strengths[..n].sort_unstable_by(|a, b| b.cmp(a));
    // CoL appendix, Genghis Khan errata: "top two" becomes "top one" when
    // exactly two civilizations remain -- the rank-`k`-th-place cutoff is
    // `strengths[k - 1]`, so this is `strengths[0]` (must be the outright
    // strongest, ties won) at n == 2 instead of the general `strengths[1]`.
    let rank_threshold = if n == 2 { 1usize } else { 2usize };
    if n < rank_threshold || mine >= strengths[rank_threshold - 1] {
        state.players[idx as usize].culture += bonus as u16;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{OneTimeDiscount, Pact, PactList, Phase, TechSlot, MAX_HAND};

    /// Headroom is only meaningful if it agrees with the band table it claims
    /// to describe, so it is checked AGAINST [`corruption`] rather than
    /// against copied-out numbers: spending exactly the headroom must leave
    /// the bill unchanged, and one more than the headroom must raise it.
    /// Written this way the test cannot drift out of step with §6.2 -- if the
    /// table ever changes, a hand-written expectation would silently become a
    /// lie, whereas this fails.
    #[test]
    fn corruption_headroom_is_exactly_the_slack_before_the_next_band() {
        for blue in 0u16..=30 {
            let h = corruption_headroom(blue);
            assert_eq!(
                corruption(blue - h),
                corruption(blue),
                "with {blue} free blue, occupying its headroom of {h} must not change the bill"
            );
            if h < CORRUPTION_HEADROOM_CAP && blue > 0 {
                assert!(
                    corruption(blue - h - 1) > corruption(blue),
                    "with {blue} free blue, occupying one past the headroom of {h} must cost more"
                );
            }
        }
    }

    /// The food-side twin, checked against [`consumption`] for the same
    /// reason (§6.4).
    #[test]
    fn consumption_headroom_is_exactly_the_population_left_before_food_costs_more() {
        for bank in 0u8..=25 {
            let h = consumption_headroom(bank);
            assert_eq!(
                consumption(bank - h),
                consumption(bank),
                "with {bank} in the yellow bank, taking its headroom of {h} must not raise the meal"
            );
            if h < CONSUMPTION_HEADROOM_CAP && bank > 0 {
                assert!(
                    consumption(bank - h - 1) > consumption(bank),
                    "with {bank} in the yellow bank, taking one past {h} must raise the meal"
                );
            }
        }
    }

    /// Standing at a band edge is the dangerous place and must read as zero
    /// slack, not as "fine, I am still in the good band" -- that confusion is
    /// the whole reason the bot could not plan up to the line.
    #[test]
    fn sitting_on_a_band_edge_reports_no_headroom_at_all() {
        for &edge in &[11u16, 6, 1] {
            assert_eq!(corruption_headroom(edge), 0, "{edge} free blue is an edge");
            assert!(corruption(edge - 1) > corruption(edge), "and one below it costs more");
        }
        for &edge in &[17u8, 13, 9, 5, 1] {
            assert_eq!(consumption_headroom(edge), 0, "a bank of {edge} is an edge");
            assert!(consumption(edge - 1) > consumption(edge), "and one below it eats more");
        }
    }

    // ---- test scaffolding: PlayerState/GameState derive no Default, so a
    // full-field literal lives here once and every test builds off it.

    fn blank_player(idx: u8) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government: CardId::NONE,
            leader: CardId::NONE,
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
            hammurabi_replaced_this_turn: false,
            replaced_leader_this_turn: false,
            trade_food_as_resource_used_this_turn: 0,
            trade_resource_as_food_used_this_turn: 0,
            churchill_used: false,
            homer_used_this_turn: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            caesar_second_politics: false,
            peeked_event: CardId::NONE,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: crate::state::OneTimeDiscount::default(),
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
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
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
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
            last_end_of_turn_culture: [None; crate::state::MAX_PLAYERS],
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

    /// `pop_cost` must pass `p.one_time_discount.pop_food` through, not the
    /// `0` it hardcoded until `state::OneTimeDiscount` existed: Python's
    /// `pop_cost` hands `pop_food_cost` the real dict, so a player holding
    /// the "Development of Civil Life" grant pays 1 food less, and pricing
    /// it 1 high is what made `apply.rs`'s `h_pop` trip its own
    /// affordability assert on replayed states.
    #[test]
    fn pop_cost_passes_the_one_time_food_discount_through() {
        let mut state = blank_state();
        state.players[0].government = card("Despotism");
        state.players[0].yellow_bank = 14; // pop_cost_base(14) == 3
        assert_eq!(pop_cost(&state, &state.players[0]), Some(3));
        state.players[0].one_time_discount.pop_food = 1;
        assert_eq!(pop_cost(&state, &state.players[0]), Some(2));
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
        assert!(increase_population(&mut p, 4, false));
        assert_eq!(p.food, 6);
        assert_eq!(p.yellow_bank, 4);
        assert_eq!(p.workers_free, 1);
    }

    #[test]
    fn increase_population_rejects_short_food_or_empty_bank() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 2;
        assert!(!increase_population(&mut p, 4, false));
        assert_eq!(p.food, 2, "a rejected attempt must not spend anything");

        p.yellow_bank = 0;
        p.food = 100;
        assert!(!increase_population(&mut p, 0, false));
    }

    /// THE REGRESSION: fixed 2026-08-05.  Development of Civil Life's
    /// `pop_food` discount is a ONE-SHOT grant (card text: "increase
    /// population ... paying 1 food ... less"), not a standing discount --
    /// `consume_one_time = true` must zero it so a second increase pays full
    /// price. Before the fix nothing ever cleared `one_time_discount`, so it
    /// silently applied to every population increase for the rest of the game.
    #[test]
    fn increase_population_consumes_the_one_time_discount_when_asked() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 10;
        p.one_time_discount.pop_food = 1;
        assert!(increase_population(&mut p, 1, true));
        assert_eq!(p.one_time_discount.pop_food, 0,
                   "consume_one_time=true must clear the discount");
    }

    /// ENGINE BUG FIX (`docs/REPLAY.md` fifth pass): Development of Civil
    /// Life's grant is ONE mutually-exclusive choice among pop/build/develop
    /// -- confirmed against real BGO play (a human who spent the discount on
    /// a technology later paid FULL price for an unrelated building, which
    /// the old "three independent discounts" model wrongly predicted should
    /// still be discounted). Spending `pop_food` must exhaust
    /// `build_resources`/`develop_science` too, not just its own field.
    #[test]
    fn increase_population_exhausts_the_whole_civil_life_grant_not_just_pop_food() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 10;
        p.one_time_discount = OneTimeDiscount { pop_food: 1, build_resources: 1, develop_science: 1 };
        assert!(increase_population(&mut p, 1, true));
        assert_eq!(p.one_time_discount, OneTimeDiscount::default(),
                   "spending the pop discount must exhaust build and develop too");
    }

    /// A population increase that never had a live Civil Life grant at all
    /// (every field already 0) must not disturb an unrelated player's own
    /// state -- trivially true here since there is nothing to clear, but
    /// pinned so a future change to the "only when live" gate can't silently
    /// start unconditionally exhausting on every ordinary Pop.
    #[test]
    fn increase_population_with_no_live_discount_leaves_the_grant_untouched() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 10;
        assert!(increase_population(&mut p, 4, true));
        assert_eq!(p.one_time_discount, OneTimeDiscount::default());
    }

    #[test]
    fn increase_population_leaves_the_discount_alone_when_not_consuming() {
        let mut p = blank_player(0);
        p.yellow_bank = 5;
        p.food = 10;
        p.one_time_discount.pop_food = 1;
        assert!(increase_population(&mut p, 0, false));
        assert_eq!(p.one_time_discount.pop_food, 1,
                   "a free increase (Ocean Liners) never looked at the \
                    discount and must not consume it");
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

    // =================================================== end-of-turn tests
    //
    // Everything below needs `effects::compute`, which dereferences
    // `p.government` unconditionally, so a player in these tests always has
    // one. Despotism is the Age A starting government: 4 civil actions, 2
    // military actions, urban limit 2, `military_hand_limit` 0 -- so the §6.7
    // military hand limit is 2 + 0 = 2 unless a test says otherwise.

    /// A blank player holding `government` -- the minimum `compute` accepts.
    fn gov_player(idx: u8, government: &str) -> PlayerState {
        let mut p = blank_player(idx);
        p.government = card(government);
        p
    }

    /// Two-player game, both on Despotism, round 1 (so §6.6 step 4 does not
    /// draw and cannot perturb a test that is about steps 2-3).
    fn duel() -> GameState {
        let mut st = blank_state();
        st.players[0] = gov_player(0, "Despotism");
        st.players[1] = gov_player(1, "Despotism");
        st
    }

    // ------------------------------------------ Trade Routes Agreement

    /// Gives player `idx` side `a_side` (`true` = side A, "1 food as 1
    /// resource"; `false` = side B, "1 resource as 1 food") of a Trade
    /// Routes Agreement with the other player in `duel()`.
    fn give_trade_routes(st: &mut GameState, idx: u8, a_side: bool) {
        let other = 1 - idx;
        let (a, b) = if a_side { (idx, other) } else { (other, idx) };
        st.players[idx as usize].pacts.push(Pact {
            card: card("Trade Routes Agreement"),
            owner: idx,
            partner: other,
            a,
            b,
        });
    }

    #[test]
    fn trade_food_as_resource_remaining_is_zero_with_no_pact() {
        let st = duel();
        assert_eq!(trade_food_as_resource_remaining(&st, &st.players[0]), 0);
        assert_eq!(trade_resource_as_food_remaining(&st, &st.players[0]), 0);
    }

    #[test]
    fn trade_food_as_resource_remaining_reads_side_a_of_the_pact() {
        let mut st = duel();
        give_trade_routes(&mut st, 0, true);
        // Side A gets "1 food as 1 resource", never the other direction.
        assert_eq!(trade_food_as_resource_remaining(&st, &st.players[0]), 1);
        assert_eq!(trade_resource_as_food_remaining(&st, &st.players[0]), 0);
        // The OTHER party, side B, gets the mirror image.
        assert_eq!(trade_food_as_resource_remaining(&st, &st.players[1]), 0);
        assert_eq!(trade_resource_as_food_remaining(&st, &st.players[1]), 1);
    }

    #[test]
    fn trade_food_as_resource_remaining_is_used_up_by_the_per_turn_counter() {
        let mut st = duel();
        give_trade_routes(&mut st, 0, true);
        st.players[0].trade_food_as_resource_used_this_turn = 1;
        assert_eq!(trade_food_as_resource_remaining(&st, &st.players[0]), 0);
    }

    #[test]
    fn trade_food_as_resource_remaining_stacks_across_two_different_partners() {
        // §5.9: "you may be party to many pacts but have only one in your
        // own area" -- a player can hold their OWN Trade Routes Agreement
        // (side A here) and ALSO be named as a party (side A again, from a
        // DIFFERENT partner's own held copy) in someone else's, stacking the
        // grant to 2 conversions this turn. `duel()` only seats two players,
        // so this exercises the stacking via two SEPARATE pact entries
        // between the same pair rather than a third seat -- `apply_pacts`
        // sums every pact `p.idx` is party to regardless of partner, so the
        // arithmetic under test is identical either way.
        let mut st = duel();
        give_trade_routes(&mut st, 0, true);
        st.players[1].pacts.push(Pact {
            card: card("Trade Routes Agreement"),
            owner: 1,
            partner: 0,
            a: 0,
            b: 1,
        });
        assert_eq!(trade_food_as_resource_remaining(&st, &st.players[0]), 2);
    }

    // -------------------------------------------------- discontent/uprising

    #[test]
    fn discontent_is_the_shortfall_against_happy_required() {
        let mut st = duel();
        st.players[0].yellow_bank = 7; // happy_required(7) == 4
        st.players[0].happy_extra = 1;
        assert_eq!(discontent(&st, &st.players[0]), 3);

        st.players[0].happy_extra = 4;
        assert_eq!(discontent(&st, &st.players[0]), 0);

        // Never negative, and `Stats::happy` is clamped to 8 by `compute`, so
        // a surplus does not become credit.
        st.players[0].happy_extra = 40;
        assert_eq!(discontent(&st, &st.players[0]), 0);
    }

    #[test]
    fn uprising_needs_strictly_more_discontent_than_free_workers() {
        let mut st = duel();
        st.players[0].yellow_bank = 7; // happy_required 4, happy 0 -> discontent 4
        assert_eq!(discontent(&st, &st.players[0]), 4);

        st.players[0].workers_free = 4;
        assert!(!uprising(&st, &st.players[0]), "equal is NOT an uprising");
        st.players[0].workers_free = 3;
        assert!(uprising(&st, &st.players[0]));
    }

    #[test]
    fn pop_cost_reads_the_bank_and_the_discount_off_state() {
        let mut st = duel();
        st.players[0].yellow_bank = 14; // pop_cost_base(14) == 3
        assert_eq!(pop_cost(&st, &st.players[0]), Some(3));
        st.players[0].yellow_bank = 0;
        assert_eq!(pop_cost(&st, &st.players[0]), None);
    }

    // ------------------------------------------------------- §6.6 ordering

    /// An uprising skips the ENTIRE production phase -- not just food and
    /// resources but science and culture too, and corruption and consumption
    /// with them. A civilization in revolt neither produces nor eats.
    #[test]
    fn end_of_turn_uprising_skips_all_of_step_3() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 7; // happy_required 4; happy 0, workers_free 0
            p.science_rate_extra = 5;
            p.culture_rate_extra = 5;
            p.blue_total = 4; // corruption(4) would be 4 if step 3b ran
            p.food = 10;
            p.resources = 3;
            p.culture = 20;
        }
        assert!(uprising(&st, &st.players[0]));
        assert!(end_of_turn(&mut st, 0));

        let p = &st.players[0];
        assert_eq!(p.science, 0, "science is production, and production was skipped");
        assert_eq!(p.culture, 20, "culture is production, and production was skipped");
        assert_eq!(p.resources, 3, "no corruption is paid during an uprising");
        assert_eq!(p.food, 10, "and nobody eats either");
        // Step 5 still runs: the turn ends even though nothing was produced.
        assert_eq!(p.civil_actions, 4);
        assert_eq!(p.military_actions, 2);
    }

    /// Step 3a before step 3d: this turn's culture income lands in the stock
    /// BEFORE the famine penalty comes out of it. Reversing the two changes
    /// the result whenever the penalty is bigger than one of the operands,
    /// because the penalty floors at zero.
    #[test]
    fn end_of_turn_scores_culture_before_the_famine_penalty() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 3; // consumption 4, happy_required 6
            p.happy_extra = 6; // ... met exactly, so no uprising
            p.blue_total = 11; // corruption(11) == 0, isolating step 3d
            p.culture_rate_extra = 10;
            p.food = 0; // no farms, no food: 4 missing -> -16 culture
        }
        assert!(!uprising(&st, &st.players[0]));
        assert!(end_of_turn(&mut st, 0));

        // Correct order: (0 + 10) - 16 -> floored to 0.
        // Scoring after the famine would give max(0, 0 - 16) + 10 == 10.
        assert_eq!(st.players[0].culture, 0);
        assert_eq!(st.players[0].food, 0);
    }

    /// Step 3b before step 3c: corruption is assessed against the blue bank
    /// as it stands BEFORE this turn's food occupies tokens in it. Producing
    /// first would push a player down a corruption band and overcharge them.
    #[test]
    fn end_of_turn_pays_corruption_before_producing_food() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 17; // consumption 0, happy_required 0
            // 5 stored resources already occupy 5 tokens, so 16 total puts
            // `blue_available` at exactly 11 -- the corruption(>=11) == 0
            // boundary, one token wide.
            p.blue_total = 16;
            p.resources = 5;
            // One farm worker: `Stats::food` == 1, which costs one blue token
            // to store and would drop `blue_available` to 10 -> corruption 2.
            p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        }
        assert_eq!(blue_available(&st.players[0]), 11);
        assert!(end_of_turn(&mut st, 0));

        assert_eq!(st.players[0].food, 1, "the farm produced");
        assert_eq!(
            st.players[0].resources, 5,
            "corruption was 0 -- assessed before the new food took a token"
        );
    }

    /// Step 3c before step 3d: a civilization eats what it just grew.
    #[test]
    fn end_of_turn_feeds_the_population_from_this_turns_harvest() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 5; // consumption 3, happy_required 5
            p.happy_extra = 5; // met exactly
            p.blue_total = 16; // corruption 0, plenty of storage
            p.culture = 20;
            p.food = 0;
            p.techs.insert(card("Agriculture"), TechSlot { workers: 4, stored: 0 });
        }
        assert!(end_of_turn(&mut st, 0));

        // Produce 4, eat 3, keep 1, no famine.
        assert_eq!(st.players[0].food, 1);
        assert_eq!(
            st.players[0].culture, 20,
            "eating before producing would have starved 3 and cost 12 culture"
        );
    }

    /// Corruption takes resources first and only then bites into food, and
    /// the food side floors at zero rather than wrapping.
    #[test]
    fn end_of_turn_corruption_falls_back_from_resources_to_food() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 17; // consumption 0, happy_required 0
            p.blue_total = 3; // blue_available 1 after the stocks below
            p.resources = 1;
            p.food = 1;
        }
        // stocks: 1 resource + 1 food = 2 tokens used of 3 -> available 1
        assert_eq!(blue_available(&st.players[0]), 1);
        assert_eq!(corruption(1), 4);
        assert!(end_of_turn(&mut st, 0));

        // 1 resource paid, 3 short, food 1 -> floored to 0 (not -2).
        assert_eq!(st.players[0].resources, 0);
        assert_eq!(st.players[0].food, 0);
    }

    #[test]
    fn end_of_turn_scores_science_and_culture_at_the_stat_rate() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 17; // consumption 0, happy_required 0
            p.blue_total = 11; // corruption 0
            p.science_rate_extra = 3;
            p.culture_rate_extra = 7;
            p.science = 2;
            p.culture = 100;
        }
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].science, 5);
        assert_eq!(st.players[0].culture, 107);
    }

    // ------------------------------------------------------ step 4 (draws)

    fn stock_military_deck(st: &mut GameState, n: usize) {
        st.round = 2;
        st.age_military = Age::A;
        st.military_deck = CardList::new();
        for i in 0..n {
            st.military_deck.push(CardId(i as u16));
        }
    }

    #[test]
    fn end_of_turn_draws_one_card_per_unspent_military_action_capped_at_three() {
        for (unspent, expect) in [(0i8, 0usize), (1, 1), (2, 2), (5, 3), (-1, 0)] {
            let mut st = duel();
            stock_military_deck(&mut st, 10);
            st.players[0].yellow_bank = 17; // no uprising, no consumption
            st.players[0].blue_total = 11; // corruption 0
            st.players[0].military_actions = unspent;
            assert!(end_of_turn(&mut st, 0));
            assert_eq!(
                st.players[0].hand_military.len(),
                expect,
                "military_actions={unspent}"
            );
            // ...and step 5 refilled the actions afterwards, from the
            // government, NOT from whatever step 4 read.
            assert_eq!(st.players[0].military_actions, 2);
        }
    }

    #[test]
    fn end_of_turn_never_draws_in_age_iv_or_on_round_one() {
        let mut st = duel();
        stock_military_deck(&mut st, 10);
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[0].military_actions = 2;
        st.age_military = Age::IV;
        assert!(end_of_turn(&mut st, 0));
        assert!(st.players[0].hand_military.is_empty(), "age IV draws nothing");

        let mut st = duel();
        stock_military_deck(&mut st, 10);
        st.round = 1;
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[0].military_actions = 2;
        assert!(end_of_turn(&mut st, 0));
        assert!(st.players[0].hand_military.is_empty(), "round 1 draws nothing");
    }

    #[test]
    fn end_of_turn_stops_drawing_when_the_deck_and_discard_are_both_empty() {
        let mut st = duel();
        stock_military_deck(&mut st, 1);
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[0].military_actions = 3;
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].hand_military.len(), 1);
    }

    // ---------------------------------------------------------- step 5

    #[test]
    fn end_of_turn_resets_the_per_turn_state() {
        let mut st = duel();
        {
            let p = &mut st.players[0];
            p.yellow_bank = 17;
            p.blue_total = 11;
            p.ca_penalty_next_turn = 3; // rebellion: 4 - 3 == 1 civil action
            p.civil_actions = 0;
            p.military_actions = 0;
            p.politics_done = true;
            p.tactic_action_used = true;
            p.hammurabi_used = true;
            p.churchill_used = true;
            p.bach_upgrade_used = true;
            p.ocean_liners_used = true;
            p.homer_used_this_turn = true;
            p.mil_discount = 4;
            p.mil_sci_discount = 3;
            p.taken_this_turn.push(card("Agriculture"));
            p.trade_food_as_resource_used_this_turn = 1;
            p.trade_resource_as_food_used_this_turn = 1;
        }
        assert!(end_of_turn(&mut st, 0));

        let p = &st.players[0];
        assert_eq!(p.civil_actions, 1, "the rebellion penalty applies once");
        assert_eq!(p.ca_penalty_next_turn, 0);
        assert_eq!(p.military_actions, 2);
        assert!(!p.politics_done);
        assert!(!p.tactic_action_used);
        assert!(!p.hammurabi_used);
        assert!(!p.churchill_used);
        assert!(!p.bach_upgrade_used);
        assert!(!p.ocean_liners_used);
        assert!(!p.homer_used_this_turn, "Homer's once-per-turn resource refreshes with every other once-per-turn flag");
        assert_eq!(p.mil_discount, 0);
        assert_eq!(p.mil_sci_discount, 0);
        assert!(p.taken_this_turn.is_empty());
        assert_eq!(
            p.trade_food_as_resource_used_this_turn, 0,
            "Trade Routes' per-turn conversion allowance refreshes with every other once-per-turn flag"
        );
        assert_eq!(p.trade_resource_as_food_used_this_turn, 0);
    }

    #[test]
    fn end_of_turn_floors_civil_actions_at_zero_under_a_big_penalty() {
        let mut st = duel();
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[0].ca_penalty_next_turn = 9; // 4 - 9 == -5
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].civil_actions, 0);
    }

    // -------------------------------------------------- step 1 (discards)

    #[test]
    fn end_of_turn_auto_discards_when_there_is_nothing_to_choose_between() {
        let mut st = duel();
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        // Despotism: 2 military actions + 0 hand limit -> limit 2.
        let bonus = card("Military Bonus (defense 2 / colonization 1)");
        for _ in 0..5 {
            st.players[0].hand_military.push(bonus);
        }
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].hand_military.len(), 2, "trimmed to the limit");
        assert_eq!(
            st.discarded_military[Age::I as usize].as_slice(),
            &[bonus, bonus, bonus],
            "and the three that went are filed under the card's own age"
        );
    }

    /// A genuine choice (two or more distinct over-limit card names) must
    /// SUSPEND the sequence: `end_of_turn` returns `false`, nothing is
    /// discarded yet, and steps 2-5 must not have run. This is the case the
    /// deleted private copy of `discard_excess_military` used to
    /// `unimplemented!()` on; routing through `interact::
    /// discard_excess_military` instead is what makes it reachable at all.
    #[test]
    fn end_of_turn_over_the_limit_with_a_real_choice_suspends() {
        let mut st = duel();
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        let a = card("Military Bonus (defense 2 / colonization 1)");
        let b = card("Military Bonus (defense 4 / colonization 2)");
        let c = card("Military Bonus (defense 6 / colonization 3)");
        st.players[0].hand_military.push(a);
        st.players[0].hand_military.push(b);
        st.players[0].hand_military.push(c);

        assert!(!end_of_turn(&mut st, 0), "a genuine choice must suspend the sequence");
        assert!(!st.pending.is_empty(), "the discard decision must be recorded as pending");
        assert_eq!(
            st.players[0].hand_military.as_slice(),
            &[a, b, c],
            "nothing is discarded until the player answers"
        );
        // Step 5 resets `military_actions` from the government (2, under
        // Despotism); still 0 here proves steps 2-5 did not run.
        assert_eq!(st.players[0].military_actions, 0, "steps 2-5 must not have run");
    }

    #[test]
    fn end_of_turn_at_the_limit_discards_nothing() {
        let mut st = duel();
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        let a = card("Military Bonus (defense 2 / colonization 1)");
        let b = card("Military Bonus (defense 4 / colonization 2)");
        st.players[0].hand_military.push(a);
        st.players[0].hand_military.push(b);
        // Two distinct cards, but exactly AT the limit -- no decision arises,
        // so the two-way case above must not fire here.
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].hand_military.as_slice(), &[a, b]);
    }

    // ------------------------------------------------------ leader bonus

    /// The magnitude comes from the card table, not from a name compare, so
    /// a second carrier of the variant would silently be ignored by
    /// `end_of_turn_leader_bonus`'s leader-only scan. Assert there is exactly
    /// one, and that it is the leader Python special-cases.
    #[test]
    fn only_genghis_khan_carries_the_top_two_strength_special() {
        let carriers: Vec<&str> = crate::cards::CARDS
            .iter()
            .filter(|c| {
                c.special.iter().any(|s| matches!(s, Special::CultureIfTopTwoStrength(_)))
            })
            .map(|c| c.name)
            .collect();
        assert_eq!(carriers, vec!["Genghis Khan"]);
        let khan = card("Genghis Khan");
        assert_eq!(khan.get().kind, crate::cards::CardType::Leader);
        assert!(khan
            .get()
            .special
            .contains(&Special::CultureIfTopTwoStrength(3)));
    }

    /// Three players, so "top two" is a real test rather than vacuous.
    fn khan_trio(mine: i16, b: i16, c: i16) -> GameState {
        let mut st = blank_state();
        st.num_players = 3;
        for i in 0..3u8 {
            st.players[i as usize] = gov_player(i, "Despotism");
        }
        st.players[0].leader = card("Genghis Khan");
        st.players[0].strength_extra = mine;
        st.players[1].strength_extra = b;
        st.players[2].strength_extra = c;
        // Neutral economy: no uprising, no consumption, no corruption.
        for i in 0..3usize {
            st.players[i].yellow_bank = 17;
            st.players[i].blue_total = 11;
        }
        st
    }

    #[test]
    fn genghis_khan_scores_while_at_or_above_second_place_in_strength() {
        // Second of three: scores.
        let mut st = khan_trio(4, 5, 3);
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 3);

        // Tied for second: still scores (the test is `>=`).
        let mut st = khan_trio(4, 5, 4);
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 3);

        // Third of three: nothing.
        let mut st = khan_trio(3, 5, 4);
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 0);

        // Strongest: scores.
        let mut st = khan_trio(9, 5, 4);
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 3);
    }

    #[test]
    fn genghis_khan_ignores_resigned_rivals() {
        // Second of three on paper (6 vs 8 vs 3) -- scores under the general
        // top-2-of-3 rule, since active() still counts all three. But once
        // the WEAKEST rival (3) resigns, only two civilizations remain, and
        // the CoL 2p errata (see `end_of_turn_leader_bonus`'s own doc)
        // collapses the requirement from "top two" to "the strongest" --
        // stricter, not the same. Khan (6) is still weaker than the
        // remaining rival (8), so the bonus must now fail. This is a real
        // distinguishing case: excluding the resigned player from the
        // ranking (correct) gives a DIFFERENT answer than leaving them in
        // would (still-scores), so a regression in either the exclusion or
        // the 2p threshold collapse flips this test.
        let mut st = khan_trio(6, 8, 3);
        st.players[2].resigned = true;
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 0);
    }

    /// Two players only: the CoL rulebook's own appendix names Genghis Khan
    /// specifically -- "In a two-player game, 'one of the two strongest'
    /// should be read as 'the strongest'. (You still win ties.)" -- so
    /// unlike `khan_trio`'s three-player case, "one of the two strongest" is
    /// NOT vacuously true here; it collapses to a strict "am I the stronger
    /// civilization" test. Traced against real game `7522205`'s
    /// culture-oracle divergence (`docs/REPLAY.md`'s culture-oracle
    /// TakeCard bucket): BGO's own "Genghis Khan scores 0 culture" line for
    /// the weaker of two 2p civilizations, which the pre-fix "vacuous 2p"
    /// code scored unconditionally (a live ENGINE bug, not a replayer
    /// artifact -- it would misscore the same way against a human).
    #[test]
    fn genghis_khan_in_a_two_player_game_needs_to_be_the_strongest_not_merely_top_two() {
        // Weaker of the two: no bonus -- the whole point of the errata.
        let mut st = duel();
        st.players[0].leader = card("Genghis Khan");
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[1].yellow_bank = 17;
        st.players[1].blue_total = 11;
        st.players[0].strength_extra = 3;
        st.players[1].strength_extra = 5;
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 0);

        // Tied: still scores ("you win ties").
        let mut st = duel();
        st.players[0].leader = card("Genghis Khan");
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[1].yellow_bank = 17;
        st.players[1].blue_total = 11;
        st.players[0].strength_extra = 4;
        st.players[1].strength_extra = 4;
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 3);

        // Outright stronger: scores.
        let mut st = duel();
        st.players[0].leader = card("Genghis Khan");
        st.players[0].yellow_bank = 17;
        st.players[0].blue_total = 11;
        st.players[1].yellow_bank = 17;
        st.players[1].blue_total = 11;
        st.players[0].strength_extra = 9;
        st.players[1].strength_extra = 4;
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 3);
    }

    #[test]
    fn no_leader_bonus_without_the_leader() {
        let mut st = khan_trio(3, 5, 4);
        st.players[0].leader = CardId::NONE;
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 0);

        let mut st = khan_trio(3, 5, 4);
        st.players[0].leader = card("Homer");
        assert!(end_of_turn(&mut st, 0));
        assert_eq!(st.players[0].culture, 0, "Homer scores no top-two bonus");
    }

    // ------------------------------------------------------ draw_military

    #[test]
    fn draw_military_pops_off_the_end_of_the_deck() {
        let mut st = blank_state();
        st.military_deck.push(CardId(7));
        st.military_deck.push(CardId(9));
        assert_eq!(draw_military(&mut st), Some(CardId(9)));
        assert_eq!(draw_military(&mut st), Some(CardId(7)));
        assert_eq!(draw_military(&mut st), None);
    }

    /// The reshuffle path, checked against CPython's own MT19937 rather than
    /// against itself: `rng.rs`'s fixture table records that
    /// `random.Random(7919).shuffle(list(range(5)))` yields `[0, 2, 4, 1, 3]`.
    /// `economy._rng` seeds with `state.seed * 7919 + state.turn`, so seed 1 /
    /// turn 0 IS that stream. Python then draws with `list.pop()` -- off the
    /// END -- which is the half of this that a "shuffle and deal" port gets
    /// backwards while still looking random.
    #[test]
    fn draw_military_reshuffles_the_discard_with_pythons_stream() {
        let mut st = blank_state();
        st.seed = 1;
        st.turn = 0;
        st.age_military = Age::A;
        for i in 0..5u16 {
            st.discarded_military[Age::A as usize].push(CardId(i));
        }
        // Reclaimed in discard order, shuffled to [0, 2, 4, 1, 3], drawn
        // back-to-front.
        let drawn: Vec<u16> =
            (0..5).map(|_| draw_military(&mut st).unwrap().0).collect();
        assert_eq!(drawn, vec![3, 1, 4, 2, 0]);
        assert!(
            st.discarded_military[Age::A as usize].is_empty(),
            "the pile must be emptied, not copied"
        );
        assert_eq!(draw_military(&mut st), None);
    }

    #[test]
    fn draw_military_only_reclaims_the_current_ages_discard() {
        let mut st = blank_state();
        st.age_military = Age::II;
        st.discarded_military[Age::A as usize].push(CardId(3));
        assert_eq!(draw_military(&mut st), None, "another age's pile is not the deck");
        assert_eq!(st.discarded_military[Age::A as usize].len(), 1);
    }
}
