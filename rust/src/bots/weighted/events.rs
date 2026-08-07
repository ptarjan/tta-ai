//! `engine/bots/weighted.py` lines 159-591: Age III scoring-event modelling
//! and the attack-target/pact-lead terms that share its "who is ahead"
//! machinery.
//!
//! ## `_EVENT_YIELD`/`_EVENT_RANKED`: tables become code, not data
//!
//! Python's `_EVENT_YIELD` is a `dict[str, (feature_key, sign)]` keyed by an
//! event block's raw JSON key, and `_event_block_value` walks `block.items()`
//! looking each one up. This port has no raw dict to walk: [`EventBlock`] is
//! already the exhaustive-against-live-data typed shape `gen_cards.py`
//! degrades that JSON into (see its own doc comment), so [`event_block_value`]
//! below reads its NAMED FIELDS directly -- the struct and the "which keys
//! exist" table are the same fact once, not two registries that could
//! disagree the way a dict-lookup-over-a-dict is. A handful of `_EVENT_YIELD`
//! entries (`strength`, `happiness`/`happy`, `yellowTokens`, `loseScience`,
//! `loseCulture`) have no field on [`EventBlock`] at all and are simply not
//! read here -- see [`event_block_value`]'s own doc comment for why that is
//! provably correct rather than a gap.
//!
//! `_EVENT_RANKED`'s six `(effect key, ranking stat, best-first)` triples
//! become an exhaustive `match` over [`Special`] in [`my_event_threat`]
//! instead of a lookup loop: `card.special` either contains
//! `Special::StrongestPlayer(block)` or it does not, so "which of the six
//! targeting keys does this card print" is a compile-time-checked dispatch,
//! not a `dict.get` per key per card.
//!
//! ## Reuse, not restatement: `crate::events`
//!
//! [`crate::events::rank_players`]/[`crate::events::RankStat`] (bumped from
//! private to `pub(crate)` alongside this port, see their own doc comments)
//! are Python's `events._rank`/`_stat_value` already ported for §5.3 event
//! resolution -- [`my_event_threat`] below reuses them rather than writing a
//! second copy of "rank players by strength/culture/happy/discontent, ties
//! broken by position". Likewise [`event_scoring_margin`] calls
//! [`crate::events::final_event_awards`] rather than restating any of the
//! fifteen "Impact of ..." formulas, exactly as Python's own docstring
//! insists on for the identical reason.
//!
//! Python asks `final_event_awards` "what would THESE candidate events pay"
//! by temporarily REBINDING `state.current_events`/`future_events` and
//! restoring them in a `finally` -- a trick that exists there only because
//! `copy.deepcopy` is expensive. [`GameState`] derives `Clone` as a flat
//! memcpy (`bots/mod.rs`'s own top doc comment), so [`event_scoring_margin`]
//! below just clones the state and mutates the copy instead: no aliasing, no
//! restore-in-`finally` footgun, and nothing to get wrong if a future edit
//! adds an early return between the rebind and the restore.

use crate::cards::{Age, CardId, EventBlock, Special};
use crate::effects;
use crate::events::{self, RankStat};
use crate::state::{CardList, ChoiceKind, GameState, Pending, PlayerState};

use super::weights::{WeightKey, Weights};

// ------------------------------------------------- Age III scoring events

/// NUMERICAL GUARD, not a model claim: the fifteen Age III scoring formulas
/// are unbounded above (`Impact of Wonders` on a five-wonder board, say), and
/// one outlier margin dominating a linear evaluator is a failure mode, not a
/// reading. Mirrors Python's `_SCORING_MARGIN_CAP`.
pub const SCORING_MARGIN_CAP: f64 = 60.0;

/// A pre-computed [`crate::bots::counting::event_pool`] result, threaded
/// through as an optional cache -- mirrors Python's `ctx.get("event_pool")`.
///
/// `None` means "compute it here"; a future per-search-node cache (`eval.rs`
/// is still a stub) that already has one for this state can pass it instead
/// of paying for `event_pool`'s deck walk again. A plain `Option<&_>`, not a
/// closure: the cache this stands in for is a genuinely-not-yet-built
/// dependency (`eval.rs`'s job), not dependency injection for its own sake.
pub type EventPool = (Vec<(CardId, u16)>, f64);

/// Event name -> how many copies of it I should expect to score, from what I
/// am ALLOWED to know. Mirrors Python's `_scoring_weights`.
///
/// Two sources, added together, matching Python exactly:
///
/// * **My own seeds, at weight 1.0** ([`my_seeds`]) -- I prepared them, so I
///   know exactly which Age III events they are.
/// * **Everything else, at `p_in_pile`** ([`crate::bots::counting::
///   event_pool`]) -- what I do NOT know, priced at the probability any one
///   unaccounted copy is sitting in a pile slot I did not seed.
///
/// A card only ever contributes here if [`events::final_scoring_block`]
/// resolves for it (mirrors Python's `effects.allPlayers` truthy check) --
/// both conditions are true for exactly the same 15 base-game events today
/// (checked 2026-08-05), so this filter is currently a no-op for the "my own
/// seeds" half (Python does not apply it there either: a card I seeded is
/// credited purely on `age_of(name) == "III"`, and `final_event_awards`
/// itself silently drops any name that is not a real scoring card either
/// way -- see [`event_scoring_margin`]'s own doc comment).
pub fn scoring_weights(state: &GameState, idx: u8, pool: Option<&EventPool>) -> Vec<(CardId, f64)> {
    let mut out: Vec<(CardId, f64)> = Vec::new();
    for name in my_seeds(state, idx) {
        if name.get().age == Age::III {
            add_weight(&mut out, name, 1.0);
        }
    }

    let computed;
    let (unknown, p) = match pool {
        Some(pool) => pool,
        None => {
            computed = crate::bots::counting::event_pool(state, idx);
            &computed
        }
    };
    if *p > 0.0 {
        for &(name, copies) in unknown {
            if name.get().age != Age::III {
                continue;
            }
            if events::final_scoring_block(name).is_none() {
                continue;
            }
            add_weight(&mut out, name, *p * f64::from(copies));
        }
    }
    out
}

/// Accumulate `amount` for `name` in a small `(CardId, f64)` pair list --
/// [`scoring_weights`]'s dict-add, `out[name] = out.get(name, 0.0) + amount`,
/// over a linear-scan collection instead of a hash map: bounded to the 15
/// base-game Age III scoring events, far below where a hash lookup would
/// ever pay for itself.
fn add_weight(out: &mut Vec<(CardId, f64)>, name: CardId, amount: f64) {
    match out.iter_mut().find(|(c, _)| *c == name) {
        Some(entry) => entry.1 += amount,
        None => out.push((name, amount)),
    }
}

/// Final-scoring culture the pending Age III events owe me, less the best
/// rival's, clamped to +/-[`SCORING_MARGIN_CAP`]. Mirrors Python's
/// `event_scoring_margin`.
///
/// Calls [`events::final_event_awards`] on a CLONE of `state` with
/// `current_events` swapped for the candidate names [`scoring_weights`]
/// supplies (and `future_events` emptied) -- so the fifteen scoring formulas
/// are never restated here and cannot drift from the rules, and the real
/// `state` the caller passed in is never touched. A candidate name that
/// turns out not to be a real scoring event (structurally impossible with
/// today's data -- see [`scoring_weights`]'s doc comment -- but not ruled
/// out by this function's own filtering alone) simply does not appear in
/// `final_event_awards`' output, contributing nothing: exactly Python's
/// behaviour, with no `try`/`except` needed to get there, because nothing
/// on this path panics for an ordinary (non-scoring) `CardId`.
///
/// Python also gates on `state.has_military`; not reproduced here for the
/// same reason `events.rs`'s own final-scoring functions already drop that
/// guard -- a card-database-completeness flag, always true for this
/// engine's always-complete 236-card table, with no `state.has_military`
/// field to read.
///
/// Zero once `state.game_over`: `game::finish_game` has by then already paid
/// these events into `p.culture`, and the decks still hold the names, so
/// counting them again would double the endgame.
pub fn event_scoring_margin(state: &GameState, idx: u8, pool: Option<&EventPool>) -> f64 {
    if state.game_over {
        return 0.0;
    }
    let n = state.num_players;
    let has_rival = (0..n).any(|i| i != idx && !state.players[i as usize].resigned);
    if !has_rival {
        return 0.0;
    }

    let weights = scoring_weights(state, idx, pool);
    if weights.is_empty() {
        return 0.0;
    }

    let mut tmp = state.clone();
    tmp.current_events = CardList::new();
    for &(name, _) in &weights {
        tmp.current_events.push(name);
    }
    tmp.future_events = CardList::new();

    let mut owed = [0.0f64; crate::state::MAX_PLAYERS];
    for (name, steps) in events::final_event_awards(&tmp) {
        let wt = weights.iter().find(|&&(c, _)| c == name).map_or(0.0, |&(_, w)| w);
        if wt == 0.0 {
            continue;
        }
        for (i, amount) in steps {
            owed[i as usize] += wt * f64::from(amount);
        }
    }

    let best_rival = (0..n)
        .filter(|&i| i != idx && !state.players[i as usize].resigned)
        .map(|i| owed[i as usize])
        .fold(f64::NEG_INFINITY, f64::max);
    let margin = owed[idx as usize] - best_rival;
    margin.clamp(-SCORING_MARGIN_CAP, SCORING_MARGIN_CAP)
}

/// One effect block, priced through the SAME weights as the real thing --
/// mirrors Python's `_event_block_value`. A pact paying +1 culture a turn is
/// worth one `culture_rate`; an event handing me 3 culture is worth three
/// `culture`. Nothing here invents a per-event constant, which is what keeps
/// `Good Harvest` and `Barbarians` from being the same move.
///
/// Reads [`EventBlock`]'s fields directly rather than Python's `_EVENT_YIELD`
/// dict lookup -- see this module's top doc comment. Five of Python's
/// `_EVENT_YIELD` entries have no counterpart here, and all five are
/// PROVABLY unreachable through this function's only call site
/// ([`my_event_threat`]'s six targeting keys plus its `gain`/`lose` blocks),
/// checked against the live data 2026-08-05:
///
/// * `strength`, `happiness`/`happy`, `yellowTokens` are never printed
///   inside a targeting/`gain`/`lose` block anywhere in the base game (only
///   inside a territory's `permanentEffects`, a completely different code
///   path `EventBlock` is not involved in at all) -- `gen_cards.py::
///   build_event_block`'s own exhaustive-against-live-data check would have
///   failed at generation time had any card printed one there, so
///   `EventBlock` correctly has no field for them.
/// * `loseScience`/`loseCulture`: zero cards print either key anywhere in
///   the base data (only the plain `science`/`culture` keys, always as
///   GAINS, are ever printed).
///
/// (Python's own `produceFood`/`produceResources` branches also only ever
/// fire on an `allPlayers` block in the live data -- a block
/// [`my_event_threat`]'s doc comment explains this function is never even
/// called with -- so `EventBlock`'s `produce_food`/`produce_resources` are a
/// deliberate omission here too, not a shape this struct lacks.)
fn event_block_value(w: &Weights, p: &PlayerState, block: &EventBlock, sign: f64) -> f64 {
    let mut total = 0.0;
    total += sign * f64::from(block.science) * w.get(WeightKey::Science);
    total += sign * f64::from(block.culture) * w.get(WeightKey::Culture);
    total += sign * f64::from(block.food) * w.get(WeightKey::FoodStock);
    total += sign * f64::from(block.resources) * w.get(WeightKey::ResourceStock);
    // `foodAndOrResources`: one block, either stock, owner's choice -- priced
    // as food, because the two share a weight sign and the choice is not
    // knowable this far out (mirrors Python's own comment on `_EVENT_YIELD`).
    total += sign * f64::from(block.food_and_or_resources) * w.get(WeightKey::FoodStock);
    total += sign * f64::from(block.blue_tokens) * w.get(WeightKey::BlueFree);
    total += sign * f64::from(block.draw_military_cards) * w.get(WeightKey::HandMilitary);
    total += sign * f64::from(block.increase_population) * w.get(WeightKey::FreeWorkers);
    total -= sign * f64::from(block.decrease_population) * w.get(WeightKey::FreeWorkers);
    // `loseAllStoredFood` carries no amount -- the amount is whatever `p` is
    // holding -- so it is priced from the board, exactly like Python's
    // `fk is None` branch.
    if block.lose_all_stored_food {
        total -= sign * f64::from(p.food) * w.get(WeightKey::FoodStock);
    }
    total
}

/// The names **I** put in the politics pile that have not resolved yet.
/// Mirrors Python's `my_seeds`.
///
/// LEGALITY IS THE WHOLE DESIGN HERE. Preparing an event places it face DOWN
/// (RULES_SPEC 5.3), so the pile's contents are hidden from everyone else --
/// but *I put it there*, so remembering my own seeds is memory, not a peek.
/// Reads `state.seeded_by` and keeps only the names whose seeder is `idx`;
/// nothing here reads a name somebody else seeded and nothing reads pile
/// ORDER, so there is no way for a deeper search to turn this into a leak.
pub fn my_seeds(state: &GameState, idx: u8) -> Vec<CardId> {
    state
        .current_events
        .as_slice()
        .iter()
        .chain(state.future_events.as_slice())
        .copied()
        .filter(|&c| state.seeded_by[c.0 as usize] == idx)
        .collect()
}

/// Summed age level of my unresolved seeds: how much event I am owed.
/// Mirrors Python's `my_seeded_pending`. A plain board count in printed
/// units, unlike [`my_event_threat`], which is priced through a [`Weights`].
pub fn my_seeded_pending(state: &GameState, idx: u8) -> f64 {
    my_seeds(state, idx).iter().map(|c| f64::from(c.level())).sum()
}

/// Live player indices in raw (not seating/turn) order, `idx` first excluded
/// by nobody -- mirrors Python's `order = [q for q in state.players if not
/// q.resigned]`, which is deliberately NOT `_order_from`'s clockwise-from-a-
/// start-index order: ties in [`events::rank_players`] break by POSITION in
/// whatever `order` it is given, and Python's tie-break here is raw index
/// order, not seating order. Reusing `events::order_from_start` (clockwise
/// from `state.start_player`) would tie-break differently and silently
/// disagree with Python on which of two equal-strength players a
/// `strongestPlayer` block lands on.
fn live_order(state: &GameState) -> Vec<u8> {
    (0..state.num_players).filter(|&i| !state.players[i as usize].resigned).collect()
}

/// Signed, DIFFERENCED value to me of the events I planted myself. Mirrors
/// Python's `my_event_threat`.
///
/// Priced against the board as it stands now, through the same weights as
/// the real thing -- so a `Good Harvest` I planted and a `Barbarians` I
/// planted stop being the same move. Differenced for the same reason
/// [`event_scoring_margin`] is: a `Barbarians` that hits the strongest
/// player is good news when the strongest player is somebody else.
///
/// Two bounds, stated rather than hidden (mirrors Python's own):
///
/// * **`allPlayers` blocks are skipped.** They hit me and every rival alike,
///   so their differential value is zero, which is what this term measures.
///   `Special::AllPlayers` is deliberately not one of this function's match
///   arms.
/// * **The ranking is TODAY's.** Whether I am still the strongest player
///   when the card actually turns up is unknowable; the board I can see is
///   the only honest estimate.
pub fn my_event_threat(state: &GameState, idx: u8, w: &Weights) -> f64 {
    let mine = my_seeds(state, idx);
    if mine.is_empty() {
        return 0.0;
    }

    let order = live_order(state);
    let denom = order.len().saturating_sub(1).max(1) as f64;
    let mut threat = 0.0;

    for name in mine {
        let special = name.get().special;

        // The six SINGULAR `_EVENT_RANKED` targeting keys: an exhaustive
        // match over `Special` stands in for Python's table-driven
        // `eff.get(key)` loop -- see this module's top doc comment.
        let mut strongest_players = None;
        let mut weakest_players = None;
        let mut gain_block = None;
        let mut lose_block = None;
        for &sp in special {
            let (stat, best, block) = match sp {
                Special::StrongestPlayer(b) => (RankStat::Strength, true, b),
                Special::WeakestPlayer(b) => (RankStat::Strength, false, b),
                Special::PlayerWithMostCulture(b) => (RankStat::Culture, true, b),
                Special::PlayerWithLeastCulture(b) => (RankStat::Culture, false, b),
                Special::PlayersWithMostHappyFaces(b) => (RankStat::Happy, true, b),
                Special::PlayersWithMostDiscontentWorkers(b) => (RankStat::Discontent, true, b),
                Special::StrongestPlayers(t) => {
                    strongest_players = Some(t);
                    continue;
                }
                Special::WeakestPlayers(t) => {
                    weakest_players = Some(t);
                    continue;
                }
                Special::Gain(b) => {
                    gain_block = Some(b);
                    continue;
                }
                Special::Lose(b) => {
                    lose_block = Some(b);
                    continue;
                }
                _ => continue,
            };
            let ranked = events::rank_players(state, &order, stat, best);
            let Some(&target) = ranked.first() else { continue };
            let v = event_block_value(w, &state.players[target as usize], &block, 1.0);
            threat += if target == idx { v } else { -v };
        }

        // The two PLURAL, count-based targeting keys (`strongestPlayers`/
        // `weakestPlayers`), which -- unlike the six above -- pull their
        // payload from a SEPARATE top-level `gain`/`lose` special rather
        // than carrying their own `EventBlock`.
        let live_idx = events::live_count_idx(state);
        if let Some(counts) = strongest_players {
            price_top_n(
                state,
                &order,
                true,
                counts[live_idx],
                &gain_block.unwrap_or(EventBlock::EMPTY),
                1.0,
                w,
                idx,
                denom,
                &mut threat,
            );
        }
        if let Some(counts) = weakest_players {
            let (block, sign) = match gain_block {
                Some(g) => (g, 1.0),
                None => (lose_block.unwrap_or(EventBlock::EMPTY), -1.0),
            };
            price_top_n(state, &order, false, counts[live_idx], &block, sign, w, idx, denom, &mut threat);
        }
    }

    threat
}

/// The top (or bottom) `count` players by strength get `block` (signed
/// `sign`), each contributing `+v` to `threat` if it lands on me or
/// `-v / denom` if it lands on a rival. Mirrors Python's
/// `for q in _rank(state, order, "strength", best)[:count]: ...` loop inside
/// `my_event_threat`'s `strongestPlayers`/`weakestPlayers` handling.
#[allow(clippy::too_many_arguments)]
fn price_top_n(
    state: &GameState,
    order: &[u8],
    best: bool,
    count: i16,
    block: &EventBlock,
    sign: f64,
    w: &Weights,
    idx: u8,
    denom: f64,
    threat: &mut f64,
) {
    if count <= 0 {
        return;
    }
    let ranked = events::rank_players(state, order, RankStat::Strength, best);
    let take = (count as usize).min(ranked.len());
    for &q in &ranked[..take] {
        let v = event_block_value(w, &state.players[q as usize], block, sign);
        *threat += if q == idx { v } else { -v / denom };
    }
}

// ------------------------------------------------------------ aggressions

/// Rival indices I am currently attacking, deduplicated. Mirrors Python's
/// `attack_targets`.
///
/// At most two sources, both public the moment they are written:
///
/// * a live `Defense` frame whose `attacker` is me -- where an aggression
///   lives between being declared and being resolved;
/// * `p.war_declared_by_me` -- a card on the table.
///
/// A fixed 2-slot result needs no heap allocation, unlike Python's
/// `sorted(set(list))`: with only two possible sources there is nothing a
/// `Vec` buys here that two `Option<u8>` fields do not, and this is called
/// once per candidate move of the search this bot runs
/// ([`attack_target_terms`]'s own doc comment on this file's search-hot
/// budget).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AttackTargets {
    first: Option<u8>,
    second: Option<u8>,
}

impl AttackTargets {
    #[inline]
    pub fn is_empty(self) -> bool {
        // `first` is always filled before `second` (see `push` below), so
        // checking only `first` is sufficient -- but spell out both here
        // rather than lean on that invariant silently: a single-source-of-
        // truth bug (checking only `first` while `push` could ever leave it
        // `None` with `second` set) is exactly the "one place says X,
        // another disagrees, nothing fails" shape this project watches for.
        self.first.is_none() && self.second.is_none()
    }

    /// Add a target, deduplicating against whatever is already here.
    fn push(&mut self, v: u8) {
        if self.first.is_none() {
            self.first = Some(v);
        } else if self.first != Some(v) {
            self.second = Some(v);
        }
    }

    pub fn iter(self) -> impl Iterator<Item = u8> {
        self.first.into_iter().chain(self.second)
    }
}

pub fn attack_targets(state: &GameState, idx: u8) -> AttackTargets {
    let mut out = AttackTargets::default();
    if let Some(Pending::Defense(d)) = state.pending.top() {
        if d.attacker == idx && d.player != idx {
            out.push(d.player);
        }
    }
    let me = &state.players[idx as usize];
    if !me.war_declared_by_me.is_none() && me.war_target != idx {
        out.push(me.war_target);
    }
    out
}

/// `target`'s culture lead over the OTHER live rivals (excluding `idx` and
/// `target` themselves), falling back to `target`'s own culture when there
/// are none. The shared "who they are, not just what the deal pays them"
/// arithmetic [`pact_partner_lead`] and [`attack_target_terms`] both need --
/// Python restates this inline in both functions; factored out here since
/// nothing about the computation differs between the two call sites.
fn culture_lead_over_others(state: &GameState, idx: u8, target: u8) -> f64 {
    let mut sum = 0i64;
    let mut count = 0i64;
    for i in 0..state.num_players {
        if i == idx || i == target || state.players[i as usize].resigned {
            continue;
        }
        sum += i64::from(state.players[i as usize].culture);
        count += 1;
    }
    let target_culture = f64::from(state.players[target as usize].culture);
    let base = if count > 0 { sum as f64 / count as f64 } else { target_culture };
    target_culture - base
}

/// Culture lead of whoever I am offering a pact to, over my other rivals.
/// Mirrors Python's `pact_partner_lead`.
///
/// `deferred_credit` (not yet ported) already prices the partner's SIDE of
/// the deal; this is the same shape of answer as [`attack_target_terms`]:
/// who they ARE, not just what the deal pays them. Handing the runaway
/// leader an alliance is a different move from handing it to last place even
/// when the printed effects are symmetric.
pub fn pact_partner_lead(state: &GameState, idx: u8) -> f64 {
    let Some(Pending::Choice(c)) = state.pending.top() else { return 0.0 };
    let ChoiceKind::PactOffer { owner, .. } = c.kind else { return 0.0 };
    if owner != idx || c.player == idx {
        return 0.0;
    }
    culture_lead_over_others(state, idx, c.player)
}

/// `(lead, weakness)` -- WHO I picked, in the two ways it matters. Mirrors
/// Python's `attack_target_terms`.
///
/// `lead` is the target's culture less the mean culture of my *other*
/// rivals: positive means I am hitting the player who is ahead, the whole
/// point of an attack in a three- or four-player game and a decision class
/// 2p cannot exercise at all (structurally 0 there -- no "other rivals" to
/// average over -- which is correct, not a gap).
///
/// `weakness` is my strength less the target's: positive means I picked
/// someone I outgun, which is what makes the attack land rather than hand
/// them a defensive card's worth of tempo. Still reads at 2p.
///
/// Both are plain board scalars in printed units -- no weight is consulted
/// here.
pub fn attack_target_terms(state: &GameState, idx: u8) -> (f64, f64) {
    let targets = attack_targets(state, idx);
    if targets.is_empty() {
        return (0.0, 0.0);
    }
    let my_strength = effects::state_stats(state, &state.players[idx as usize]).strength;
    let mut lead = 0.0;
    let mut weakness = 0.0;
    for t in targets.iter() {
        lead += culture_lead_over_others(state, idx, t);
        weakness += f64::from(my_strength - effects::state_stats(state, &state.players[t as usize]).strength);
    }
    (lead, weakness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{CardId, NUM_CARDS};
    use crate::state::{
        CardList, ChoiceOption, Defense, Keyword, OneTimeDiscount, OptionList, PactList,
        PendingStack, Phase, Queue, Tableau, MAX_PLAYERS, NOT_SEEDED, ROW_SIZE,
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
            hammurabi_replaced_this_turn: false,
            replaced_leader_this_turn: false,
            churchill_used: false,
            bach_upgrade_used: false,
            ocean_liners_used: false,
            caesar_double_politics_used: false,
            skip_next_politics: false,
            caesar_second_politics: false,
            peeked_event: CardId::NONE,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: OneTimeDiscount::default(),
            resigned: false,
        }
    }

    /// `num_players` real seats, the rest filled with an inert resigned
    /// placeholder so `live_order`/rival loops never see them.
    fn multi_player_state(num_players: u8, players_in: &[PlayerState]) -> GameState {
        let filler = || {
            let mut p = blank_player(9, card("Despotism"));
            p.resigned = true;
            p
        };
        let mut players = [filler(), filler(), filler(), filler()];
        for (i, p) in players_in.iter().enumerate() {
            players[i] = p.clone();
        }
        GameState {
            num_players,
            seed: 0,
            players,
            current: 0,
            turn: 1,
            round: 2,
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
            seeded_by: [NOT_SEEDED; NUM_CARDS],
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
            pending: PendingStack::new(),
            queue: Queue::new(),
        }
    }

    fn weights_with(key: WeightKey, value: f64) -> Weights {
        let mut w = Weights::default();
        w.set(key, value);
        w
    }

    // -------------------------------------------------------- my_seeds

    #[test]
    fn my_seeds_keeps_only_my_own_seeded_names() {
        let mut state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        state.current_events.push(card("Impact of Industry"));
        state.current_events.push(card("Impact of Balance"));
        state.future_events.push(card("Impact of Colonies"));
        state.seeded_by[card("Impact of Industry").0 as usize] = 0;
        state.seeded_by[card("Impact of Balance").0 as usize] = 1; // rival's
        state.seeded_by[card("Impact of Colonies").0 as usize] = 0;
        let mut mine = my_seeds(&state, 0);
        mine.sort_by_key(|c| c.0);
        let mut expect = [card("Impact of Industry"), card("Impact of Colonies")];
        expect.sort_by_key(|c| c.0);
        assert_eq!(mine, expect);
    }

    #[test]
    fn my_seeded_pending_sums_age_levels() {
        let mut state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        state.current_events.push(card("Impact of Industry")); // Age III -> level 3
        state.seeded_by[card("Impact of Industry").0 as usize] = 0;
        assert_eq!(my_seeded_pending(&state, 0), 3.0);
    }

    // ------------------------------------------------------ scoring_weights

    #[test]
    fn scoring_weights_credits_my_own_seed_at_one() {
        let mut state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        state.current_events.push(card("Impact of Industry"));
        state.seeded_by[card("Impact of Industry").0 as usize] = 0;
        let weights = scoring_weights(&state, 0, Some(&(Vec::new(), 0.0)));
        assert_eq!(weights, vec![(card("Impact of Industry"), 1.0)]);
    }

    #[test]
    fn scoring_weights_ignores_a_seed_that_is_not_age_iii() {
        // Politics of Strength is an Age I event -- seeding it must not
        // register as an owed scoring event.
        let mut state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        state.current_events.push(card("Politics of Strength"));
        state.seeded_by[card("Politics of Strength").0 as usize] = 0;
        let weights = scoring_weights(&state, 0, Some(&(Vec::new(), 0.0)));
        assert!(weights.is_empty());
    }

    // --------------------------------------------------- event_scoring_margin

    #[test]
    fn event_scoring_margin_prices_my_seed_against_the_best_rival() {
        // "Impact of Industry": culturePerResourceProducedByMines: 1. Player 0
        // has 2 mine-workers (2 resources/turn via mine_resources), player 1
        // has none -- so player 0 is owed 2, player 1 owed 0, margin = 2.
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 2, stored: 0 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1]);
        state.current_events.push(card("Impact of Industry"));
        state.seeded_by[card("Impact of Industry").0 as usize] = 0;

        assert_eq!(event_scoring_margin(&state, 0, Some(&(Vec::new(), 0.0))), 2.0);
        // Player 1 never seeded this event and it is not in their own pile
        // scan -- an EMPTY pool means they have no information about it at
        // all, and the margin must be exactly 0.0, not the negation: this is
        // the whole point of `my_seeds` vs. `event_pool` being two different
        // sources (see this file's top doc comment on `_scoring_weights`).
        assert_eq!(event_scoring_margin(&state, 1, Some(&(Vec::new(), 0.0))), 0.0);
        // Give player 1 the SAME certainty via the "unknown pool" route
        // instead (as if `counting::event_pool` had placed it in their pile
        // with probability 1) -- only then does the margin flip sign, since
        // now they are pricing the identical event at the identical weight.
        let pool = (vec![(card("Impact of Industry"), 1u16)], 1.0);
        assert_eq!(event_scoring_margin(&state, 1, Some(&pool)), -2.0);
    }

    #[test]
    fn event_scoring_margin_is_zero_once_the_game_is_over() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Bronze"), crate::state::TechSlot { workers: 2, stored: 0 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1]);
        state.current_events.push(card("Impact of Industry"));
        state.seeded_by[card("Impact of Industry").0 as usize] = 0;
        state.game_over = true;
        assert_eq!(event_scoring_margin(&state, 0, Some(&(Vec::new(), 0.0))), 0.0);
    }

    // ------------------------------------------------------- my_event_threat

    #[test]
    fn my_event_threat_is_zero_with_no_seeds() {
        let state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        assert_eq!(my_event_threat(&state, 0, &Weights::default()), 0.0);
    }

    #[test]
    fn my_event_threat_prices_a_strongest_player_block_through_food_stock() {
        // "Foray" prints `strongestPlayers: {2p: 1, ...}` + `gain:
        // {foodAndOrResources: 3}` -- the strongest player(s) gain 3
        // food-or-resources. Give player 0 more strength via
        // `strength_extra` so it is the (sole, at 2p) target, and price
        // through `food_stock` (`foodAndOrResources`'s feature, see
        // `event_block_value`'s doc comment).
        let mut p0 = blank_player(0, card("Despotism"));
        p0.strength_extra = 10;
        let p1 = blank_player(1, card("Despotism"));
        let mut state = multi_player_state(2, &[p0, p1]);
        state.current_events.push(card("Foray"));
        state.seeded_by[card("Foray").0 as usize] = 0;

        let gain_block = card("Foray")
            .get()
            .special
            .iter()
            .find_map(|sp| match sp {
                Special::Gain(b) => Some(*b),
                _ => None,
            })
            .expect("Foray prints a gain block");
        let w = weights_with(WeightKey::FoodStock, 2.0);
        let expected = f64::from(gain_block.food_and_or_resources) * 2.0;
        assert_eq!(my_event_threat(&state, 0, &w), expected);
        // The rival's threat is the negation -- but player 1 seeded nothing,
        // so they have no threat term of their own to report at all.
        assert_eq!(my_event_threat(&state, 1, &w), 0.0);
    }

    // ------------------------------------------------------- attack_targets

    #[test]
    fn attack_targets_reads_a_live_defense_frame_i_opened() {
        let mut state = multi_player_state(3, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism")), blank_player(2, card("Despotism"))]);
        state.pending.push(Pending::Defense(Defense {
            player: 1,
            attacker: 0,
            card: card("Aggression: Enslave"),
            atk: 3,
            dfn: 1,
            spent: 0,
            budget: 2,
        }));
        let targets = attack_targets(&state, 0);
        assert_eq!(targets.iter().collect::<Vec<_>>(), vec![1]);
        // Not the defender's own view -- they are not attacking anyone.
        assert!(attack_targets(&state, 1).is_empty());
    }

    #[test]
    fn attack_targets_reads_a_declared_war() {
        let mut state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        state.players[0].war_declared_by_me = card("Aggression: Enslave");
        state.players[0].war_target = 1;
        assert_eq!(attack_targets(&state, 0).iter().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn attack_target_terms_lead_is_zero_at_two_players() {
        let mut state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        state.players[0].war_declared_by_me = card("Aggression: Enslave");
        state.players[0].war_target = 1;
        state.players[1].culture = 40;
        let (lead, _weakness) = attack_target_terms(&state, 0);
        assert_eq!(lead, 0.0); // no "other rivals" to average over at 2p
    }

    #[test]
    fn attack_target_terms_lead_is_positive_when_i_hit_the_leader() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.war_declared_by_me = card("Aggression: Enslave");
        p0.war_target = 1;
        let mut p1 = blank_player(1, card("Despotism"));
        p1.culture = 50; // the target, well ahead
        let mut p2 = blank_player(2, card("Despotism"));
        p2.culture = 10; // the "other rival" p1's lead is measured against
        let state = multi_player_state(3, &[p0, p1, p2]);
        let (lead, _weakness) = attack_target_terms(&state, 0);
        assert_eq!(lead, 40.0); // 50 - 10
    }

    #[test]
    fn attack_target_terms_weakness_is_my_strength_minus_the_targets() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.war_declared_by_me = card("Aggression: Enslave");
        p0.war_target = 1;
        p0.strength_extra = 5;
        let p1 = blank_player(1, card("Despotism"));
        let state = multi_player_state(2, &[p0, p1]);
        let (_lead, weakness) = attack_target_terms(&state, 0);
        assert_eq!(weakness, 5.0);
    }

    // ------------------------------------------------------ pact_partner_lead

    #[test]
    fn pact_partner_lead_is_the_partners_culture_lead_over_the_rest() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.culture = 30;
        let mut p2 = blank_player(2, card("Despotism"));
        p2.culture = 10;
        let mut state = multi_player_state(3, &[p0, p1, p2]);
        let mut options = OptionList::new();
        options.push(ChoiceOption::Word(Keyword::Accept));
        options.push(ChoiceOption::Word(Keyword::Refuse));
        state.pending.push(Pending::Choice(crate::state::Choice {
            player: 1, // the partner deciding
            kind: ChoiceKind::PactOffer { owner: 0, card: card("Peace Treaty"), a: 0, b: 1 },
            options,
        }));
        assert_eq!(pact_partner_lead(&state, 0), 20.0); // 30 - 10
        // Not my offer -- from the partner's own idx there is no pact I own.
        assert_eq!(pact_partner_lead(&state, 1), 0.0);
    }

    #[test]
    fn pact_partner_lead_ignores_an_unrelated_pending_frame() {
        let state = multi_player_state(2, &[blank_player(0, card("Despotism")), blank_player(1, card("Despotism"))]);
        assert_eq!(pact_partner_lead(&state, 0), 0.0);
    }
}
