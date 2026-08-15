//! The mid-move decision queue -- the port of `engine/interact.py`.
//!
//! The rest of the engine is a single-player-at-a-time move generator: the
//! player to move is `state.current`. Several rules break that assumption --
//! colonization auctions (§11), aggression defense (§5.4.4), pact offers
//! (§5.9) and "each player chooses" event effects (§5.3) -- so the state
//! carries a STACK of open decisions and a FIFO of deferred sub-effects. The
//! same machinery also carries decisions that ARE the current player's but
//! arrive mid-effect: an action card's ordered free action (§3.11), and
//! raid/annex/infiltrate targeting.
//!
//! The two containers and their types live in `state.rs` ([`PendingStack`],
//! [`Queue`], [`Pending`], [`QueueItem`]) -- see the "the decision queue"
//! section there for why each is shaped the way it is, and for the bound on
//! each array and where that bound came from. This module is the behaviour:
//! what moves an open decision admits ([`pending_moves`]), what applying one
//! does ([`apply_pending`]), and the fifteen choice resolvers / fifteen
//! deferred sub-effects Python dispatches through `_CHOICE` / `_QUEUE_ITEMS`.
//!
//! ## Python's two dispatch tables are exhaustive `match`es here
//!
//! `interact._CHOICE` and `interact._QUEUE_ITEMS` are `dict.get(tag)` lookups
//! whose miss path is `if fn is None: return` -- a decision whose tag is not
//! in the table is silently dropped, and nothing fails. That is this
//! project's recurring bug class verbatim. [`ChoiceKind`] and [`QueueItem`]
//! have one variant per key of those two tables and every dispatch below is
//! an exhaustive `match`, so a new decision kind is a compile error rather
//! than a no-op.
//!
//! One difference is deliberate and is a FINDING, not an omission:
//! `_QUEUE_ITEMS` registers sixteen handlers, and one of them --
//! `"gains"` (`_q_gains` -> `events.apply_gains`) -- has NO producer.
//! `grep -rn '"tag": "gains"' engine/` (2026-08-05) finds only the registry
//! entry itself; every `interact.enqueue` call site in `engine/` uses one of
//! the other fifteen tags. It is dead code in Python, so [`QueueItem`] has no
//! variant for it. If a future event ever needs it, the compile error is at
//! the enqueue site, which is where it belongs.
//!
//! ## Nothing here is stubbed
//!
//! The auction, the colonization force, the defense decision, all fifteen
//! choice resolvers and all fifteen deferred sub-effects are ported; the
//! two that hand back to the turn loop ([`QueueItem::EndOfTurn`] and
//! `finish_take_row`'s `deal_row`) call `game.rs` for real. (A third,
//! `_q_auto_skip_politics`, USED to have a [`QueueItem`] variant here too --
//! removed 2026-08-14, `game::start_turn`'s own doc comment has the reason:
//! it was closing real humans' Politics Phase before their own logged
//! `Move::PolPass` could land, an `IllegalMove: PolPass` bug on 54 BGO
//! games, not a faithful port of anything BGO's client actually does.)
//!
//! One thing used to be blocked one step PAST this module: `combat::
//! finish_aggression`'s success branch needed `events::apply_gains` and the
//! per-card dict payloads `gen_cards.py` collapsed to payload-less
//! `Special`s. Both landed 2026-08-05 (`events.rs`, plus generator work for
//! `takeFromOpponent`/`destroyUrbanBuildings`) -- the four queue items that
//! branch enqueues (`Raid`/`Annex`/`Infiltrate`/`LosePop`) were already
//! resolved here, waiting for it, and now are reached for real. Nothing else
//! here needs `events.rs`: the two gain blocks that DO reach this module --
//! an event's `choose` option and a territory's `immediateEffects` -- are
//! closed vocabularies, applied directly.

use crate::cards::{Age, CardId, CardType};
use crate::combat;
use crate::costs;
use crate::economy;
use crate::effects;
use crate::legal;
use crate::moves::{Move, MoveList};
use crate::state::{
    CardList, Choice, ChoiceKind, ChoiceOption, Colonize, Defense, GainOption, GameState,
    Keyword, OptionList, Pending, PlayerState, QueueItem, MAX_FORCE, MAX_HAND, MAX_PLAYERS,
    MAX_TABLEAU, ROW_SIZE,
};

// ------------------------------------------------------------- plumbing

/// The decision that owns the next move (`interact.top`).
#[inline]
pub fn top(state: &GameState) -> Option<&Pending> {
    state.pending.top()
}

/// Moves the open decision admits. Mirrors `interact.pending_moves`, and is
/// what `legal::legal_moves` returns while `state.pending` is non-empty.
///
/// Panics with nothing pending, exactly as Python's `state.pending[-1]`
/// raises: the caller is required to have checked.
pub fn pending_moves(state: &GameState) -> MoveList {
    let mut out = MoveList::new();
    match state.pending.top().expect("pending_moves with an empty pending stack") {
        Pending::Choice(c) => {
            for i in 0..c.options.len() {
                out.push(Move::Choose { n: i as u8 });
            }
        }
        Pending::Auction(a) => {
            // `bid_pass` FIRST, then ascending bids -- the order is the
            // contract (`DESIGN.md`: the bots break ties by index).
            out.push(Move::BidPass);
            let ceiling = max_force(state, &state.players[a.player as usize]);
            for n in (a.bid as i32 + 1)..=ceiling {
                debug_assert!(n <= u8::MAX as i32, "bid {n} does not fit in Move::Bid");
                out.push(Move::Bid { n: n as u8 });
            }
        }
        Pending::Defense(d) => {
            out.push(Move::DefendDone);
            if d.spent < d.budget {
                let p = &state.players[d.player as usize];
                let mut buf = [CardId::NONE; MAX_HAND];
                // Python: `sorted(set(d.hand_military))` -- by NAME, not by
                // `discard_options`' defense ordering. The two orders differ
                // and this one is the aggression-defense move list.
                let n = sorted_unique_by_name(p.hand_military.as_slice(), &mut buf);
                for &id in &buf[..n] {
                    out.push(Move::Defend { card: id });
                }
            }
        }
        Pending::Colonize(c) => {
            for m in colonize_moves(state, c).as_slice() {
                out.push(*m);
            }
        }
    }
    out
}

/// Apply a response to the open decision. Mirrors `interact.apply_pending`.
///
/// Python's tail is `run_queue(state, rng); effects.invalidate(state)`; there
/// is no stats cache in this port (`effects::state_stats` just computes), so
/// only the first half exists here.
pub fn apply_pending(state: &mut GameState, mv: Move) {
    match mv {
        Move::Choose { n } => {
            let pend = state.pending.pop().expect("apply_pending with an empty pending stack");
            let Pending::Choice(choice) = pend else {
                panic!("Choose against a {pend:?} decision -- move/decision mismatch")
            };
            resolve_choice(state, &choice, n as usize);
        }
        Move::Bid { .. } | Move::BidPass => auction_move(state, mv),
        Move::Defend { .. } | Move::DefendDone => defense_move(state, mv),
        Move::SendUnit { .. } | Move::SendBonus { .. } | Move::SendDiscard { .. } | Move::SendDone => {
            colonize_move(state, mv)
        }
        other @ Move::Take { .. } | other @ Move::Build { .. } | other @ Move::Develop { .. } | other @ Move::Upgrade { .. } | other @ Move::WonderStep { .. } | other @ Move::Pop | other @ Move::PopFree | other @ Move::Revolution { .. } | other @ Move::PlayLeader { .. } | other @ Move::PlayAction { .. } | other @ Move::Destroy { .. } | other @ Move::PlayTactic { .. } | other @ Move::CopyTactic { .. } | other @ Move::Aggression { .. } | other @ Move::War { .. } | other @ Move::OfferPact { .. } | other @ Move::CancelPact { .. } | other @ Move::PrepareEvent { .. } | other @ Move::RemoveLeaderYellow | other @ Move::ColumbusColonize { .. } | other @ Move::Barbarossa { .. } | other @ Move::BachTheater { .. } | other @ Move::TradeFoodAsResource | other @ Move::TradeResourceAsFood | other @ Move::Churchill { .. } | other @ Move::EndTurn | other @ Move::PolPass | other @ Move::Resign => panic!("{other:?} is not a response to an open decision"),
    }
    run_queue(state);
}

/// Ask `player` to pick one of `options`. Mirrors `interact.push_choice`.
///
/// With `auto`, a SINGLE option is taken immediately (no decision is opened)
/// and an empty option list is a no-op -- so callers never have to
/// special-case "there is nothing to choose from". Returns whether a decision
/// is now pending, which is the answer `discard_excess_military` and
/// `_q_lose_pop` branch on.
pub fn push_choice(
    state: &mut GameState,
    player: u8,
    kind: ChoiceKind,
    options: OptionList,
    auto: bool,
) -> bool {
    if options.is_empty() {
        return false;
    }
    let choice = Choice { player, kind, options };
    if auto && choice.options.len() == 1 {
        resolve_choice(state, &choice, 0);
        return false;
    }
    state.pending.push(Pending::Choice(choice));
    true
}

/// Defer a sub-effect, resolved FIFO once the decision stack empties (§5.3).
#[inline]
pub fn enqueue(state: &mut GameState, item: QueueItem) {
    state.queue.push_back(item);
}

/// Resolve deferred sub-effects until one of them needs a decision.
/// Mirrors `interact.run_queue`: the guard is `pending` EMPTY, so a queued
/// item that opens a decision suspends the drain rather than stacking.
pub fn run_queue(state: &mut GameState) {
    while state.pending.is_empty() {
        let Some(item) = state.queue.pop_front() else { return };
        // Python's `_run_item` skips an item belonging to a resigned player
        // entirely -- before dispatch, so the sub-effect never happens.
        if state.players[item.player() as usize].resigned {
            continue;
        }
        run_item(state, item);
    }
}

// ---------------------------------------------------------- small helpers

/// Distinct cards, sorted by NAME. Python spells this `sorted(set(...))` over
/// a list of name strings, which is exactly this: `CardId::name` is the sort
/// key, never the id, because ids are table order and Python sorts text.
fn sorted_unique_by_name(cards: &[CardId], buf: &mut [CardId]) -> usize {
    let mut n = 0;
    for &c in cards {
        if !buf[..n].contains(&c) {
            buf[n] = c;
            n += 1;
        }
    }
    buf[..n].sort_unstable_by_key(|id| id.name());
    n
}

/// Order-preserving de-duplication (`interact._distinct`): two copies of one
/// card are ONE move.
fn distinct(cards: &[CardId], buf: &mut [CardId]) -> usize {
    let mut n = 0;
    for &c in cards {
        if !buf[..n].contains(&c) {
            buf[n] = c;
            n += 1;
        }
    }
    n
}

/// Players in clockwise order from `first`, skipping the resigned (§5.3
/// tie-break). Mirrors `engine/events.py::_order_from`; duplicated here
/// rather than imported because `events.rs` does not exist yet and this is
/// four lines. Delete in favour of `events::order_from` when it lands.
fn order_from(state: &GameState, first: u8) -> ([u8; MAX_PLAYERS], usize) {
    let mut out = [0u8; MAX_PLAYERS];
    let mut n = 0;
    let np = state.num_players as usize;
    for i in 0..np {
        let idx = (first as usize + i) % np;
        if !state.players[idx].resigned {
            out[n] = idx as u8;
            n += 1;
        }
    }
    (out, n)
}

/// An option list built from a slice of cards, in the slice's order.
fn card_options(cards: &[CardId]) -> OptionList {
    let mut out = OptionList::new();
    for &c in cards {
        out.push(ChoiceOption::Card(c));
    }
    out
}

// ------------------------------------------------- military hand discards

/// What a military card adds to defense when spent defending (§5.4.4).
///
/// The three Military Bonus cards print 2/4/6; every other military card is
/// worth the flat +1 of a face-down card. Zero means "not printed" -- no
/// base-game military card prints an explicit `defenseBonus: 0` (checked
/// 2026-08-05), so the sentinel is unambiguous, and it is the same reading
/// `effects::army_value` already gives `Card::obsolete_strength`.
/// [`defense_move`] is the authority and calls this, so the two can never
/// disagree.
pub fn defense_points(id: CardId) -> i32 {
    let bonus = id.get().effects.defense_bonus as i32;
    if bonus != 0 {
        bonus
    } else {
        1
    }
}

/// Distinct military cards, LEAST defensively useful first.
///
/// Presentation order only -- every bot is free to score the options with its
/// own evaluator, and the search bots do. It is load-bearing anyway: the
/// weighted-family evaluator is documented-blind to military card identity
/// beyond age, so same-age options tie and every argmax in the project falls
/// back to option 0. Ordering by the engine's own defense arithmetic makes
/// that fallback "discard the card that defends least", which is the rule's
/// intent and is derived, not invented. Name is the secondary key, so the
/// order is total and deterministic.
pub fn discard_options(hand: &[CardId], buf: &mut [CardId]) -> usize {
    let mut n = 0;
    for &c in hand {
        if !buf[..n].contains(&c) {
            buf[n] = c;
            n += 1;
        }
    }
    buf[..n].sort_unstable_by_key(|id| (defense_points(*id), id.name()));
    n
}

/// §6.6 step 1: discard down to the military hand limit (§6.7).
///
/// The player CHOOSES which card goes -- the rulebook's only end-of-turn
/// decision. Returns `true` when a choice is now pending and the caller must
/// suspend; `false` when the hand is legal.
///
/// Idempotent: it re-reads the limit every call, so the resume path is just
/// "call it again". The limit cannot move while the loop runs -- it is read
/// off cards in play, not off the hand.
///
/// This is the ONLY implementation of §6.6 step 1. `economy.rs` briefly
/// carried a second copy, written while `state.pending` did not exist, which
/// `unimplemented!()`d the multi-option case; games panicked within about
/// four rounds, because the military hand limit is `military_actions +
/// militaryHandLimit` and no base-game government prints `militaryHandLimit`
/// at all -- so the limit is 2 under Despotism while a player draws up to 3
/// cards a turn, and the over-limit case is the COMMON case, not the corner.
/// That copy was deleted (commit `d2824de`) and `economy::end_of_turn` now
/// calls this function, matching Python, where §6.6 step 1 likewise lives in
/// `interact.py` and is called from `economy.end_of_turn`. Two copies of one
/// rule with nothing failing when they disagree is the bug class DESIGN.md
/// exists to close; do not add a third.
pub fn discard_excess_military(state: &mut GameState, idx: u8) -> bool {
    loop {
        let limit = {
            let p = &state.players[idx as usize];
            let s = effects::state_stats(state, p);
            s.military_actions + s.military_hand_limit
        };
        let p = &state.players[idx as usize];
        if crate::debugflags::replay_debug_all() {
            eprintln!(
                "DEBUG discard_excess_military: idx={idx} hand_military_len={} limit={limit} \
                 military_actions={} military_hand_limit={} hand={:?}",
                p.hand_military.len(),
                effects::state_stats(state, p).military_actions,
                effects::state_stats(state, p).military_hand_limit,
                p.hand_military.as_slice().iter().map(|&c| c.name()).collect::<Vec<_>>(),
            );
        }
        if p.hand_military.len() as i32 <= limit {
            return false;
        }
        let mut buf = [CardId::NONE; MAX_HAND];
        let n = discard_options(p.hand_military.as_slice(), &mut buf);
        if n == 0 {
            return false;
        }
        let options = card_options(&buf[..n]);
        if push_choice(state, idx, ChoiceKind::DiscardMilitary, options, true) {
            return true;
        }
        // One distinct name left: `push_choice` resolved it without a
        // decision (§6.6 is still satisfied -- there was nothing to choose
        // between), so go round again.
    }
}

// --------------------------------------------------------------- choices

/// Dispatch one answered `choice`. Mirrors `interact._resolve_choice`, with
/// its `_CHOICE.get(tag)` miss path replaced by an exhaustive `match`.
fn resolve_choice(state: &mut GameState, choice: &Choice, idx: usize) {
    let opt = choice
        .options
        .get(idx)
        .unwrap_or_else(|| panic!("choose({idx}) is out of range for {} options", choice.options.len()));
    let p = choice.player;
    match choice.kind {
        ChoiceKind::GainBlock => {
            let ChoiceOption::Gain(g) = opt else { wrong_option(&choice.kind, opt) };
            // `events.apply_gains` over a `choose` branch. The base game's
            // only such block prints food/resources and nothing else (see
            // `state::GainOption`), and both keys are plain gains.
            let pl = &mut state.players[p as usize];
            economy::gain_food(pl, g.food.max(0) as u16);
            economy::gain_resources(pl, g.resources.max(0) as u16);
        }
        ChoiceKind::FreeCivil { discount } => {
            let ChoiceOption::Move(mv) = opt else { wrong_option(&choice.kind, opt) };
            crate::apply::apply_free_civil_move(state, p, mv, discount as i32);
        }
        ChoiceKind::FoodOrRes { n } => {
            let ChoiceOption::Word(w) = opt else { wrong_option(&choice.kind, opt) };
            let pl = &mut state.players[p as usize];
            match w {
                Keyword::Food => {
                    economy::gain_food(pl, n.max(0) as u16);
                }
                Keyword::Resources => {
                    economy::gain_resources(pl, n.max(0) as u16);
                }
                other @ Keyword::Stop | other @ Keyword::Skip | other @ Keyword::Science | other @ Keyword::Accept | other @ Keyword::Refuse | other @ Keyword::Leader | other @ Keyword::Wonder => panic!("food_or_res offered {other:?}"),
            }
        }
        ChoiceKind::FreeBuild => {
            // Python: "skip" or no free worker or a card no longer in play
            // are all silent no-ops.
            let ChoiceOption::Card(id) = opt else { return };
            let pl = &mut state.players[p as usize];
            if pl.workers_free == 0 {
                return;
            }
            if let Some(slot) = pl.techs.get_mut(id) {
                slot.workers += 1;
                pl.workers_free -= 1;
            }
        }
        ChoiceKind::DestroyOwn => {
            // Destroy one of your own farms/mines/urban buildings (no refund).
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            let pl = &mut state.players[p as usize];
            if let Some(slot) = pl.techs.get_mut(id) {
                if slot.workers > 0 {
                    slot.workers -= 1;
                    pl.workers_free += 1;
                }
            }
        }
        ChoiceKind::LosePop => {
            // "Lose 1 population" when there is no unused worker (FAQ p.15):
            // the token goes back to the BANK, not to `workers_free`.
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            let pl = &mut state.players[p as usize];
            if let Some(slot) = pl.techs.get_mut(id) {
                if slot.workers > 0 {
                    slot.workers -= 1;
                    pl.yellow_bank += 1;
                }
            }
        }
        ChoiceKind::LoseColony => {
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            lose_colony(&mut state.players[p as usize], id);
        }
        ChoiceKind::FlipWonder => {
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            let pl = &mut state.players[p as usize];
            if pl.completed_wonders.contains(id) && !pl.flipped_wonders.contains(id) {
                pl.flipped_wonders.push(id);
            }
        }
        ChoiceKind::DiscardMilitary => {
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            if state.players[p as usize].hand_military.remove_first(id) {
                economy::discard_military(state, id);
            }
        }
        ChoiceKind::Raid { victim, loot } => {
            // Attacker picks the urban building to destroy, or declines this
            // bracket outright (`Keyword::Stop` -- see `QueueItem::Raid`'s own
            // doc on why "up to N" needs a real decline option; Terrorism's
            // mandatory destroy never offers `Stop`, so this arm never sees it
            // for that case).
            if matches!(opt, ChoiceOption::Word(Keyword::Stop)) {
                return;
            }
            // Attacker picks the urban building to destroy.
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            let ok = match state.players[victim as usize].techs.get_mut(id) {
                Some(slot) if slot.workers > 0 => {
                    slot.workers -= 1;
                    true
                }
                _ => false,
            };
            if !ok {
                return;
            }
            state.players[victim as usize].workers_free += 1;
            if loot {
                // PRINTED build cost, halved and rounded up.
                let printed = id.get().resource_cost as u16;
                economy::gain_resources(&mut state.players[p as usize], printed.div_ceil(2));
            }
        }
        ChoiceKind::Annex { victim } => {
            // §5.4: the colony's PERMANENT bonus changes hands.
            let ChoiceOption::Card(id) = opt else { wrong_option(&choice.kind, opt) };
            lose_colony(&mut state.players[victim as usize], id);
            let perm = &id.get().effects;
            let pl = &mut state.players[p as usize];
            pl.colonies.push(id);
            crate::apply::grant_yellow(pl, perm.yellow_tokens as i32);
            pl.blue_total = (pl.blue_total as i32 + perm.blue_tokens as i32).max(0) as u8;
        }
        ChoiceKind::Infiltrate { victim, per } => {
            // Remove the rival's leader or unfinished wonder; `per` culture
            // per level of the removed card.
            let ChoiceOption::Word(w) = opt else { wrong_option(&choice.kind, opt) };
            match w {
                Keyword::Leader => {
                    let leader = state.players[victim as usize].leader;
                    if leader.is_none() {
                        return;
                    }
                    crate::apply::on_leave_play(&mut state.players[victim as usize], leader);
                    state.players[p as usize].culture += per as u16 * leader.level() as u16;
                    economy::discard_civil(state, leader);
                    state.players[victim as usize].leader = CardId::NONE;
                }
                Keyword::Wonder => {
                    let wonder = state.players[victim as usize].wonder;
                    if wonder.is_none() {
                        return;
                    }
                    state.players[p as usize].culture += per as u16 * wonder.level() as u16;
                    economy::discard_civil(state, wonder);
                    state.players[victim as usize].wonder = CardId::NONE;
                    // Python's `victim.wonder = None` drops the whole
                    // `WonderInProgress`, steps included.
                    state.players[victim as usize].wonder_steps = 0;
                }
                other @ Keyword::Stop | other @ Keyword::Skip | other @ Keyword::Science | other @ Keyword::Food | other @ Keyword::Resources | other @ Keyword::Accept | other @ Keyword::Refuse => panic!("infiltrate offered {other:?}"),
            }
        }
        ChoiceKind::PactOffer { owner, card, a, b } => {
            let ChoiceOption::Word(w) = opt else { wrong_option(&choice.kind, opt) };
            match w {
                Keyword::Accept => {
                    // ASSIGNMENT, and it is the printed rule, not a bug -- it
                    // has been reported as one. 2015 rulebook: "You can be a
                    // party to more than one pact, but you can have only one
                    // pact in YOUR PLAY AREA. If you offer a new pact and the
                    // player accepts, any pact in your play area is
                    // automatically cancelled." So agreeing a new pact as
                    // OWNER replaces the one you own, and that is all this
                    // does. Pacts you are party to but do not own live in the
                    // other owner's list and are untouched.
                    let mut pacts = crate::state::PactList::new();
                    let pact = crate::state::Pact { card, owner, partner: p, a, b };
                    pacts.push(pact);
                    state.players[owner as usize].pacts = pacts;
                    // §5.9: "it applies immediately" -- top up BOTH parties'
                    // LIVE per-turn military-action pool right now, not just
                    // the state-stats formula their own next end-of-turn
                    // reset would already pick up (`apply::on_pact_accepted`'s
                    // own doc comment).
                    crate::apply::on_pact_accepted(state, &pact);
                }
                Keyword::Refuse => {
                    // Refusal returns the card to the offerer's hand.
                    state.players[owner as usize].hand_military.push(card);
                }
                other @ Keyword::Stop | other @ Keyword::Skip | other @ Keyword::Science | other @ Keyword::Food | other @ Keyword::Resources | other @ Keyword::Leader | other @ Keyword::Wonder => panic!("pact_offer offered {other:?}"),
            }
        }
        ChoiceKind::TakeRow { budget } => {
            // International Agreement: spend up to N civil actions taking
            // cards. Note the take is NOT charged to `pay_ca` -- `budget` is
            // its own pool, and Python calls `take_card`, not `_h_take`.
            match opt {
                ChoiceOption::Word(Keyword::Stop) => finish_take_row(state),
                ChoiceOption::Slot(slot) => {
                    let cost = costs::take_cost(state, &state.players[p as usize], slot as usize);
                    // CoL p.12: action cards taken via International
                    // Agreement may be played the SAME turn -- see
                    // `apply::take_card_from_international_agreement`'s own
                    // doc for the citation and why this is the one caller
                    // that must not feed `taken_this_turn`.
                    crate::apply::take_card_from_international_agreement(state, p, slot as usize);
                    // CoL p.12: the forfeit of the player's next political
                    // action is the cost of actually USING this privilege --
                    // taking a card here is the only place that happens, so
                    // this is the only place that pays for it (`events.rs`'s
                    // own doc comment on `optional_take_cards_with_civil_
                    // actions` has the full citation and the real-game
                    // evidence for why this moved off the reveal-time
                    // handler). Idempotent: setting it again on a second or
                    // third card taken this same session is a no-op.
                    state.players[p as usize].skip_next_politics = true;
                    offer_take_row(state, p, budget - cost as i16);
                }
                other @ ChoiceOption::Card(_) | other @ ChoiceOption::Move(_) | other @ ChoiceOption::Gain(_) | other @ ChoiceOption::Word(_) => wrong_option(&choice.kind, other),
            }
        }
        ChoiceKind::WarTech { victim, budget } => match opt {
            ChoiceOption::Word(Keyword::Science) => {
                take_war_science(state, p, victim, budget as i32)
            }
            ChoiceOption::Card(id) => {
                let cost = id.get().science_cost as i32;
                steal_special_tech(state, p, victim, id);
                offer_war_tech(state, p, victim, budget as i32 - cost);
            }
            other @ ChoiceOption::Slot(_) | other @ ChoiceOption::Move(_) | other @ ChoiceOption::Gain(_) | other @ ChoiceOption::Word(_) => wrong_option(&choice.kind, other),
        },
        ChoiceKind::PlunderSplit { victim } => {
            let ChoiceOption::Gain(g) = opt else { wrong_option(&choice.kind, opt) };
            // The defender's loss is unconditional (§5.4.6 losses are never
            // blue-token limited -- see `FoodOrResSplit`'s `lose` arm below,
            // which this mirrors). The attacker's gain IS blue-token
            // limited, so a full bank can still cap what actually crosses
            // over even though the defender's share is already gone; that
            // half-degenerate outcome is inherited from the pre-choice
            // fixed-formula code this choice replaced rather than
            // introduced by this fix.
            let (food, res) = (g.food.max(0) as u16, g.resources.max(0) as u16);
            let v = &mut state.players[victim as usize];
            v.food = v.food.saturating_sub(food);
            v.resources = v.resources.saturating_sub(res);
            let pl = &mut state.players[p as usize];
            economy::gain_food(pl, food);
            economy::gain_resources(pl, res);
        }
        ChoiceKind::FoodOrResSplit { lose } => {
            let ChoiceOption::Gain(g) = opt else { wrong_option(&choice.kind, opt) };
            let (food, res) = (g.food.max(0) as u16, g.resources.max(0) as u16);
            let pl = &mut state.players[p as usize];
            if lose {
                // §5.4.6: losses are never blue-token limited -- the option
                // itself already respects what the player's own banks can
                // supply (`plunder_split_options`, reused), so a plain
                // saturating subtraction is safe and matches Plunder's own
                // victim-side arm above.
                pl.food = pl.food.saturating_sub(food);
                pl.resources = pl.resources.saturating_sub(res);
            } else {
                // A gain: blue-token limited per bank, exactly like
                // Plunder's own attacker-side arm above -- see
                // `food_or_res_gain_options`'s doc for why the option list
                // itself is not pre-capped.
                economy::gain_food(pl, food);
                economy::gain_resources(pl, res);
            }
        }
    }
}

/// A `ChoiceOption` shape that the `ChoiceKind` that produced the list cannot
/// have generated. Unreachable unless a producer and its resolver disagree,
/// which is exactly the thing worth failing loudly on.
fn wrong_option(kind: &ChoiceKind, opt: ChoiceOption) -> ! {
    panic!("{kind:?} cannot resolve option {opt:?} -- producer/resolver mismatch")
}

// ------------------------------------------- War over Technology spoils

/// AN OPTION INDEX, NOT AN AMOUNT OF SCIENCE. `WAR_TECH_SCIENCE_IDX = 0`
/// reads like a rules value ("a war over technology pays zero science"),
/// which is the opposite of what it says; the name carries `_IDX` for that
/// reason.
///
/// Index of the "take the rest as science" option inside a `war_tech`
/// choice. It is FIRST on purpose: every argmax in this project breaks a tie
/// to the lowest index, and index 0 being "science" makes the blind fallback
/// exactly the behaviour the engine had before the choice existed.
pub const WAR_TECH_SCIENCE_IDX: u8 = 0;

/// Blue technologies the victor of a War over Technology may steal now.
///
/// The card: *"The victor takes science equal to the strength advantage, or
/// takes special (blue) technologies of the same total cost."* The rules that
/// decide WHICH are on offer are Code of Laws p.3 and the FAQ p.8:
///
/// * *"the victor takes the card from the defeated civilization's PLAY
///   AREA"* -- only cards the loser has in play, not in hand, not from the
///   card row.
/// * *"A player cannot steal a special technology that is the same as one he
///   or she already has in play or in hand."*
/// * The steal is paid out of the strength advantage at the card's cost, so a
///   technology the remaining advantage cannot cover is not on offer.
/// * PRINTED cost, not `costs::tech_cost`: Code of Laws p.4, *"If the effect
///   refers to the cost of a card, use the cost printed on the card, ignoring
///   any modifiers."*
///
/// Most expensive first, then by name: a total order, and the option that
/// spends the most of the advantage leads.
pub fn war_tech_options(state: &GameState, victor: u8, loser: u8, budget: i32) -> OptionList {
    let mut buf = [CardId::NONE; MAX_TABLEAU];
    let mut n = 0;
    let v = &state.players[victor as usize];
    for (id, _) in state.players[loser as usize].techs.iter() {
        if id.kind() != CardType::SpecialTech {
            continue;
        }
        if v.techs.has(id) || v.hand_civil.contains(id) {
            continue;
        }
        if id.get().science_cost as i32 > budget {
            continue;
        }
        buf[n] = id;
        n += 1;
    }
    buf[..n].sort_unstable_by_key(|id| (-(id.get().science_cost as i32), id.name()));
    card_options(&buf[..n])
}

/// §5.7: hand the victor of a War over Technology its spoils decision.
///
/// Mixing is explicitly legal -- FAQ p.8's "some or all" -- so this offers
/// ONE steal at a time and re-offers with the advantage reduced by what the
/// card cost, until the victor takes the remainder as science or there is
/// nothing left it may steal.
pub fn war_tech_spoils(state: &mut GameState, victor: u8, loser: u8, budget: i32) {
    offer_war_tech(state, victor, loser, budget);
}

fn offer_war_tech(state: &mut GameState, victor: u8, loser: u8, budget: i32) {
    let opts = war_tech_options(state, victor, loser, budget);
    if opts.is_empty() {
        // Nothing stealable: there is no decision, so do not manufacture one.
        take_war_science(state, victor, loser, budget);
        return;
    }
    // "science" is prepended, so it lands at `WAR_TECH_SCIENCE_IDX`.
    let mut full = OptionList::new();
    full.push(ChoiceOption::Word(Keyword::Science));
    for &o in opts.as_slice() {
        full.push(o);
    }
    push_choice(state, victor, ChoiceKind::WarTech { victim: loser, budget: budget as i16 }, full, false);
}

/// The other half of the card, and the cap FAQ p.8 puts on it: *"The attacker
/// cannot take more Science points than the loser has to lose"*, so
/// `min(budget, loser.science)`.
pub fn take_war_science(state: &mut GameState, victor: u8, loser: u8, budget: i32) {
    let take = budget.min(state.players[loser as usize].science as i32);
    if take <= 0 {
        return;
    }
    state.players[loser as usize].science -= take as u16;
    state.players[victor as usize].science += take as u16;
}

/// Move one blue technology from the loser's play area into the victor's.
///
/// Code of Laws p.3: *"the victor takes the card from the defeated
/// civilization's play area and puts it into his or her own play area"*, and
/// *"If you steal a special technology of the same type as one that you have
/// in play, you keep the higher level card in play and discard the other."*
/// That second sentence is §7.6's one-per-icon rule, which
/// `apply::put_special_in_play` already implements for the develop path -- so
/// the steal routes through it instead of restating it.
///
/// It is NOT a develop: no science is paid and `on_develop` (Leonardo /
/// Newton / Einstein) does not fire, because nobody played the card.
/// `on_enter_play` / `on_leave_play` DO fire on both sides, because the card
/// really does leave one play area and enter the other.
fn steal_special_tech(state: &mut GameState, victor: u8, loser: u8, id: CardId) {
    crate::apply::on_leave_play(&mut state.players[loser as usize], id);
    state.players[loser as usize].techs.remove(id);
    crate::apply::put_special_in_play(state, victor, id);
}

/// Settle any outstanding War over Technology spoils by taking the BEST
/// AFFORDABLE option, not by hardcoding science.
///
/// For LOOKAHEAD callers only: they resolve a declared war on a scratch state
/// and score the position immediately, so a decision left hanging would price
/// the war at nothing at all. There is no real chooser here to consult, so
/// this cannot reproduce the victor's actual preference -- but it no longer
/// has to settle for the floor either.
///
/// `war_tech_options`' own doc comment fixes a total order on the steal
/// options: "most expensive first", i.e. the option that spends the most of
/// the advantage leads. Option 0 (`WAR_TECH_SCIENCE_IDX`) is science; option
/// 1, when present, is that most-expensive affordable steal. Taking it
/// whenever it exists prices the war at a permanent blue technology instead
/// of one-time science, which is the ceiling `war_tech_options` can offer for
/// that budget -- a tighter bound than always taking the floor.
///
/// This WAS "science unconditionally": every search that priced a declared
/// War over Technology saw only the science branch and never the steal, so
/// it saw the floor of the card and never its ceiling, biasing every bot
/// pricing that war one-sided low. Preferring the steal removes that bias in
/// the direction the FAQ actually allows ("some or all", p.8) rather than
/// hardcoding the option the card lists first.
pub fn settle_war_spoils(state: &mut GameState) {
    // Not a `while let Some(...) = state.pending.top() { ... }`: `top()`
    // borrows `state.pending`, and the loop body's `apply_pending(state,
    // ...)` needs `state` mutably. Extracting only the `usize` we need
    // (`n_options`) out of the match, then breaking the immutable borrow
    // before the mutable call, is what makes this compile at all -- clippy's
    // `while let` rewrite is not just style here, it would move the borrow
    // in a way this function can't satisfy.
    #[allow(clippy::while_let_loop)]
    loop {
        let n_options = match state.pending.top() {
            Some(Pending::Choice(Choice { kind: ChoiceKind::WarTech { .. }, options, .. })) => {
                options.len()
            }
            _ => break,
        };
        // Index 0 is always science (`WAR_TECH_SCIENCE_IDX`); index 1, when
        // present, is the priciest affordable steal (`war_tech_options`'
        // order). Prefer it -- see the doc comment above for why.
        let n = if n_options > 1 { 1 } else { WAR_TECH_SCIENCE_IDX };
        apply_pending(state, Move::Choose { n });
    }
}

// -------------------------------------------------------- Aggression: Plunder

/// Every way to split Aggression: Plunder's "take up to `printed` food
/// and/or resources" (§5.4.6) between the two banks, capped at what the
/// defender actually has.
///
/// `total` is `min(printed, defender.resources + defender.food)`: the exact
/// cap the pre-choice code already enforced (`events::food_or_resources`'
/// two-step drain), which this fix does not touch -- ONLY the mix was wrong,
/// not the amount. Every split that reaches `total` is offered; a split that
/// reaches less than `total` is never in the attacker's interest (more is
/// strictly better, since there is no reason for the victim to keep any of
/// it) and was never reachable before this choice existed either, so it is
/// not manufactured here.
///
/// Ordered most resources first, matching the "resources first" order the
/// pre-choice code always took: option 0 is that old default, so the
/// convention every other choice in this module follows (index 0 is the
/// blind fallback) holds here too.
fn plunder_split_options(defender: &PlayerState, printed: i32) -> OptionList {
    let mut opts = OptionList::new();
    let total = printed.min(defender.resources as i32 + defender.food as i32).max(0);
    if total == 0 {
        return opts;
    }
    // `r` is resources taken; food taken is always `total - r`, so this is a
    // total order over splits, not a search: the range is exactly the `r`
    // values for which BOTH banks can supply their share of `total`.
    let r_max = (defender.resources as i32).min(total);
    let r_min = (total - defender.food as i32).max(0);
    let mut r = r_max;
    while r >= r_min {
        opts.push(ChoiceOption::Gain(GainOption { food: (total - r) as i16, resources: r as i16 }));
        r -= 1;
    }
    opts
}

/// Open the attacker's Plunder split decision (FAQ p.7: the split is the
/// ATTACKER's choice, not a fixed "resources first" order). A single
/// feasible split -- e.g. the defender holds only one of the two banks --
/// auto-resolves rather than manufacturing a decision with one option.
pub fn offer_plunder_split(state: &mut GameState, attacker: u8, defender: u8, printed: i32) {
    let opts = plunder_split_options(&state.players[defender as usize], printed);
    push_choice(state, attacker, ChoiceKind::PlunderSplit { victim: defender }, opts, true);
}

// -------------------------------------------------------- Raiders / Foray

/// Foray's gain-direction enumeration for §5.3's `foodAndOrResources` split
/// (`ChoiceKind::FoodOrResSplit { lose: false }`, opened from
/// `QueueItem::FoodOrResSplit` below): every way to split `total` between
/// food and resources, with NO cap at option-build time -- deliberately
/// mirroring [`plunder_split_options`]'s own attacker-gain precedent just
/// above (see that field's doc on `ChoiceKind::PlunderSplit` in `state.rs`):
/// blue-token availability caps what actually lands, per bank, at
/// RESOLUTION (`economy::gain_food`/`gain_resources`, in `resolve_choice`'s
/// own `FoodOrResSplit` arm), not by filtering the option list here.
/// Raiders' LOSS direction needs no sibling function: it reuses
/// `plunder_split_options` directly against the player's OWN banks, since
/// "how much of `total` can this player's two banks actually supply" is
/// exactly the same question Plunder already answers for a victim's.
fn food_or_res_gain_options(total: i16) -> OptionList {
    let mut opts = OptionList::new();
    let total = total.max(0);
    if total == 0 {
        return opts;
    }
    let mut r = total;
    while r >= 0 {
        opts.push(ChoiceOption::Gain(GainOption { food: total - r, resources: r }));
        r -= 1;
    }
    opts
}

// -------------------------------------------------- International Agreement

fn offer_take_row(state: &mut GameState, p: u8, budget: i16) {
    let mut opts = OptionList::new();
    for slot in 0..ROW_SIZE {
        if state.card_row[slot].is_none() {
            continue;
        }
        let player = &state.players[p as usize];
        if costs::take_cost(state, player, slot) > budget as i32 {
            continue;
        }
        if costs::can_take(state, player, slot, Some(budget as i32)) {
            opts.push(ChoiceOption::Slot(slot as u8));
        }
    }
    if opts.is_empty() {
        finish_take_row(state);
        return;
    }
    opts.push(ChoiceOption::Word(Keyword::Stop));
    push_choice(state, p, ChoiceKind::TakeRow { budget }, opts, false);
}

/// CoL p.12: replenish afterwards WITHOUT discarding the first slots.
fn finish_take_row(state: &mut GameState) {
    // The compaction is portable and is done first, exactly as Python does:
    // surviving cards slide left, the tail becomes empty slots.
    let mut kept = [CardId::NONE; ROW_SIZE];
    let mut n = 0;
    for slot in 0..ROW_SIZE {
        if !state.card_row[slot].is_none() {
            kept[n] = state.card_row[slot];
            n += 1;
        }
    }
    state.card_row = kept;
    crate::game::deal_row(state);
}

// ------------------------------------------------------ queued sub-effects

/// Dispatch one deferred sub-effect. Mirrors `interact._run_item`, with
/// `_QUEUE_ITEMS.get(tag)`'s silent miss path replaced by an exhaustive
/// `match`.
fn run_item(state: &mut GameState, item: QueueItem) {
    match item {
        QueueItem::Choose { player, options } => {
            let mut opts = OptionList::new();
            for &g in options.as_slice() {
                opts.push(ChoiceOption::Gain(g));
            }
            push_choice(state, player, ChoiceKind::GainBlock, opts, true);
        }
        QueueItem::FreeCivil { player, kind, discount, revolt_ok } => {
            // Offer the concrete moves that satisfy an action card's order.
            let k = legal::free_action_kind_of(kind);
            let moves = legal::free_action_moves(
                state,
                &state.players[player as usize],
                k,
                discount as i32,
                revolt_ok,
            );
            let mut opts = OptionList::new();
            for &m in moves.as_slice() {
                opts.push(ChoiceOption::Move(m));
            }
            push_choice(state, player, ChoiceKind::FreeCivil { discount }, opts, true);
        }
        QueueItem::CardGains { player, gains } => apply_card_gains(state, player, gains),
        QueueItem::FreeBuild { player, spec } => {
            // Event free build: pick a card of the allowed kind, or decline.
            if state.players[player as usize].workers_free == 0 {
                return;
            }
            let s = effects::state_stats(state, &state.players[player as usize]);
            let mut buf = [CardId::NONE; MAX_TABLEAU];
            let n = {
                let p = &state.players[player as usize];
                let mut n = 0;
                for (id, _) in p.techs.iter() {
                    buf[n] = id;
                    n += 1;
                }
                buf[..n].sort_unstable_by_key(|id| id.name());
                n
            };
            let mut opts = OptionList::new();
            for &id in &buf[..n] {
                let card = id.get();
                if !card.kind.takes_workers() {
                    continue;
                }
                // Python matches the spec's `card` against `baseName` OR the
                // disambiguated name, so an age-suffixed printing still
                // matches its printed name.
                if !spec.card.is_none()
                    && card.base_name != spec.card.get().base_name
                    && id != spec.card
                {
                    continue;
                }
                if let Some(age) = spec.age {
                    if card.age != age {
                        continue;
                    }
                }
                if let Some(kind) = spec.kind {
                    if card.kind != kind {
                        continue;
                    }
                }
                if card.kind.is_urban()
                    && costs::urban_count(&state.players[player as usize], card.kind) >= s.urban_limit
                {
                    continue;
                }
                if spec.cost > 0 && state.players[player as usize].resources < spec.cost {
                    continue;
                }
                opts.push(ChoiceOption::Card(id));
            }
            if opts.is_empty() {
                return;
            }
            opts.push(ChoiceOption::Word(Keyword::Skip));
            push_choice(state, player, ChoiceKind::FreeBuild, opts, false);
        }
        QueueItem::DestroyOwn { player } => {
            let opts = worker_holding_options(state, player, |k| k.is_urban() || k.is_production());
            push_choice(state, player, ChoiceKind::DestroyOwn, opts, true);
        }
        QueueItem::LosePop { player, n } => {
            for i in 0..n {
                if state.players[player as usize].workers_free > 0 {
                    let p = &mut state.players[player as usize];
                    p.workers_free -= 1;
                    p.yellow_bank += 1;
                    continue;
                }
                let opts = worker_holding_options(state, player, CardType::takes_workers);
                if push_choice(state, player, ChoiceKind::LosePop, opts, true) {
                    // Remaining losses are re-queued IN FRONT of the decision.
                    let left = n - i - 1;
                    if left > 0 {
                        state.queue.push_front(QueueItem::LosePop { player, n: left });
                    }
                    return;
                }
            }
        }
        QueueItem::FoodOrResSplit { player, amount, lose } => {
            // §5.3's `foodAndOrResources` gain/lose block (Raiders, Foray):
            // the TARGETED player's own split choice -- see
            // `ChoiceKind::FoodOrResSplit`'s doc in `state.rs`. Loss reuses
            // `plunder_split_options` against the player's OWN banks (the
            // same "what can actually be paid" windowing Plunder already
            // does for a victim's); gain has no such cap at option-build
            // time (`food_or_res_gain_options`'s own doc).
            let opts = if lose {
                plunder_split_options(&state.players[player as usize], amount as i32)
            } else {
                food_or_res_gain_options(amount)
            };
            push_choice(state, player, ChoiceKind::FoodOrResSplit { lose }, opts, true);
        }
        QueueItem::LoseColony { player } => {
            let mut buf = [CardId::NONE; 8];
            let n = sorted_unique_by_name(state.players[player as usize].colonies.as_slice(), &mut buf);
            let opts = card_options(&buf[..n]);
            push_choice(state, player, ChoiceKind::LoseColony, opts, true);
        }
        QueueItem::FlipWonder { player, ages } => {
            let mut buf = [CardId::NONE; 8];
            let mut n = 0;
            {
                let p = &state.players[player as usize];
                for &w in p.completed_wonders.as_slice() {
                    if p.flipped_wonders.contains(w) {
                        continue;
                    }
                    if ages & (1 << (w.get().age as u8)) == 0 {
                        continue;
                    }
                    if !buf[..n].contains(&w) {
                        buf[n] = w;
                        n += 1;
                    }
                }
            }
            buf[..n].sort_unstable_by_key(|id| id.name());
            let opts = card_options(&buf[..n]);
            push_choice(state, player, ChoiceKind::FlipWonder, opts, true);
        }
        QueueItem::DiscardMilitary { player, n } => {
            for i in 0..n {
                let mut buf = [CardId::NONE; MAX_HAND];
                let k = discard_options(
                    state.players[player as usize].hand_military.as_slice(),
                    &mut buf,
                );
                if k == 0 {
                    return;
                }
                let opts = card_options(&buf[..k]);
                if push_choice(state, player, ChoiceKind::DiscardMilitary, opts, true) {
                    let left = n - i - 1;
                    if left > 0 {
                        state.queue.push_front(QueueItem::DiscardMilitary { player, n: left });
                    }
                    return;
                }
            }
        }
        // Resume §6.6 after the player's discard decision. The end-of-turn
        // sequence is the one place a decision interrupts a PHASE TRANSITION
        // rather than an action, so the continuation rides the deferred queue
        // like any other sub-effect and the turn advances from there.
        QueueItem::EndOfTurn { player } => crate::game::resume_end_turn(state, player),
        QueueItem::Raid { player, victim, max_age, no_loot } => {
            let max_lv = max_age as u8;
            let mut buf = [CardId::NONE; MAX_TABLEAU];
            let mut n = 0;
            for (id, slot) in state.players[victim as usize].techs.iter() {
                if slot.workers > 0 && id.kind().is_urban() && id.level() <= max_lv {
                    buf[n] = id;
                    n += 1;
                }
            }
            buf[..n].sort_unstable_by_key(|id| id.name());
            let mut opts = card_options(&buf[..n]);
            // A real Raid card's printed text is "destroy UP TO N urban
            // buildings" (RULES_SPEC.md line 82/98) -- `combat::
            // finish_aggression` enqueues one of these per printed age
            // bracket (Raid II/III carry two), and each bracket is
            // independently optional, not just the card as a whole: a human
            // attacker can destroy fewer than the max even with an eligible
            // target still standing. Terrorism's identical-shaped forced
            // destruction (`events.rs`'s `destroy_one_urban_building_of_
            // each_opponent`) is NOT optional and carries no loot -- `no_loot`
            // is already the one field that tells the two apart at this call
            // site, so it doubles as "is this bracket declinable" here. Without
            // this, `push_choice`'s own `auto`+`len()==1` path (below) would
            // force-destroy a lone remaining candidate the moment only one is
            // left, which is exactly wrong for an "up to" choice -- confirmed
            // on human game `7522647` (Orange's starting `Philosophy`, force-
            // destroyed by Raid II's second bracket with no journal record of
            // it; `analysis/worker_notes_2026-08-14/stuckfix__STUCKFIX.txt`).
            if !no_loot {
                opts.push(ChoiceOption::Word(Keyword::Stop));
            }
            push_choice(state, player, ChoiceKind::Raid { victim, loot: !no_loot }, opts, true);
        }
        QueueItem::Annex { player, victim } => {
            let mut buf = [CardId::NONE; 8];
            let n = sorted_unique_by_name(state.players[victim as usize].colonies.as_slice(), &mut buf);
            let opts = card_options(&buf[..n]);
            push_choice(state, player, ChoiceKind::Annex { victim }, opts, true);
        }
        QueueItem::Infiltrate { player, victim, per } => {
            let mut opts = OptionList::new();
            if !state.players[victim as usize].leader.is_none() {
                opts.push(ChoiceOption::Word(Keyword::Leader));
            }
            if !state.players[victim as usize].wonder.is_none() {
                opts.push(ChoiceOption::Word(Keyword::Wonder));
            }
            push_choice(state, player, ChoiceKind::Infiltrate { victim, per }, opts, true);
        }
        QueueItem::TakeRow { player, budget } => offer_take_row(state, player, budget),
    }
}

/// Worker-holding cards of `p`, name-sorted -- the option list shape shared
/// by `destroy_own` and `lose_pop`. Python spells both `sorted(n for n, t in
/// p.techs.items() if t.workers > 0 and ...)`.
fn worker_holding_options(
    state: &GameState,
    player: u8,
    accept: impl Fn(CardType) -> bool,
) -> OptionList {
    let mut buf = [CardId::NONE; MAX_TABLEAU];
    let mut n = 0;
    for (id, slot) in state.players[player as usize].techs.iter() {
        if slot.workers > 0 && accept(id.kind()) {
            buf[n] = id;
            n += 1;
        }
    }
    buf[..n].sort_unstable_by_key(|id| id.name());
    card_options(&buf[..n])
}

/// The gain half of a yellow action card (§3.11). Mirrors `engine/actions.py::
/// apply_card_gains`, including its ordering: the flat gains land first and
/// `gainFoodOrResources` opens its choice LAST.
pub fn apply_card_gains(state: &mut GameState, player: u8, gains: crate::state::CardGains) {
    {
        let p = &mut state.players[player as usize];
        p.science += gains.science.max(0) as u16;
        p.culture += gains.culture.max(0) as u16;
        economy::gain_food(p, gains.food.max(0) as u16);
        economy::gain_resources(p, gains.resources.max(0) as u16);
        for _ in 0..gains.population.max(0) {
            if p.yellow_bank > 0 {
                p.yellow_bank -= 1;
                p.workers_free += 1;
            }
        }
    }
    if gains.food_or_resources != 0 {
        let mut opts = OptionList::new();
        opts.push(ChoiceOption::Word(Keyword::Food));
        opts.push(ChoiceOption::Word(Keyword::Resources));
        push_choice(
            state,
            player,
            ChoiceKind::FoodOrRes { n: gains.food_or_resources },
            opts,
            false,
        );
    }
}

// ------------------------------------------------------------ colonization

/// Every military unit in play, one entry per worker (§11.3), weakest first.
///
/// Python builds this from `p.techs.items()` (tableau BUILD order) and then
/// stable-sorts by `(strength, level)`, so cards tying on both keep build
/// order. `sort_by_key` is Rust's stable sort, so it reproduces that; a
/// `sort_unstable_by_key` here would reorder ties and change which unit a
/// `moves[0]` bot sacrifices.
pub fn unit_pool(p: &PlayerState) -> CardList<MAX_FORCE> {
    let mut out = CardList::<MAX_FORCE>::new();
    for (id, slot) in p.techs.iter() {
        if id.kind().is_unit() && slot.workers > 0 {
            for _ in 0..slot.workers {
                out.push(id);
            }
        }
    }
    out.as_mut_slice().sort_by_key(|id| (id.get().effects.strength, id.level()));
    out
}

/// Military bonus cards in hand, cheapest colonization value first. Hand
/// order among equals, for the same stability reason as [`unit_pool`].
pub fn bonus_pool(p: &PlayerState) -> CardList<MAX_FORCE> {
    let mut out = CardList::<MAX_FORCE>::new();
    for &id in p.hand_military.as_slice() {
        if id.kind() == CardType::Bonus {
            out.push(id);
        }
    }
    out.as_mut_slice().sort_by_key(|id| id.get().effects.colonization_bonus);
    out
}

/// Whether `p`'s active leader is the named leader. Duplicated from (not
/// imported from) `legal.rs`/`apply.rs`/`costs.rs`'s own private copies --
/// see `costs.rs`'s "A note on leader identity" for why this is a name
/// compare against `Card.name` rather than a `Special` dispatch.
#[inline]
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

/// James Cook may discard at most this many military cards per colonization.
/// Mirrors `engine/interact.py::COOK_DISCARDS`. Deliberately NOT read off
/// `Special::ColonizeDiscardUpTo2MilitaryCardsForBonus`'s payload -- that
/// value is the +1 BONUS per discard (`colonizeDiscardUpTo2MilitaryCardsFor
/// Bonus: 1` in the data), and the cap of two is a data wart: it is spelled
/// inside the effect KEY's own name instead of carried beside it, so unlike
/// the +1 it cannot be queried (`engine/interact.py`'s own comment on this,
/// ded32dd). `costs.rs`/`effects.rs` have no test pinning this constant
/// against the key the way `tests/test_leader_action_abilities.py` does on
/// the Python side; there is nothing more machine-readable to pin it to here.
const COOK_DISCARDS: i32 = 2;

/// James Cook's `colonizeDiscardUpTo2MilitaryCardsForBonus` payload: the
/// colony-force bonus per discard. `0` for anyone else -- callers only ever
/// pass a non-empty `discards` slice while Cook is the leader (`cook_pool`
/// is the only producer of `dpool`, and it is empty for every other leader).
fn cook_bonus_per_discard(p: &PlayerState) -> i32 {
    if p.leader.is_none() {
        return 0;
    }
    p.leader
        .get()
        .special
        .iter()
        .find_map(|s| match s {
            crate::cards::Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(v) => Some(*v as i32),
            crate::cards::Special::A(_) | crate::cards::Special::AllPlayers(_) | crate::cards::Special::B(_) | crate::cards::Special::BestTheaterDoubleCulture | crate::cards::Special::BothPlayers(_) | crate::cards::Special::BuildDiscount(_) | crate::cards::Special::CancelledIfPartiesAttackEachOther | crate::cards::Special::CannotPlayAggressionOrWar | crate::cards::Special::CivilActionBackOnTechDevelop(_) | crate::cards::Special::CivilActionUpgradeUrbanBuildingToTheater | crate::cards::Special::ColonyImmediateBonusApplies | crate::cards::Special::ColonyPermanentBonusTransfers | crate::cards::Special::ComboFoodDiscount(_) | crate::cards::Special::ComboResourceDiscount(_) | crate::cards::Special::Condition(_) | crate::cards::Special::CultureFirstColony(_) | crate::cards::Special::CultureIfTopTwoStrength(_) | crate::cards::Special::CultureOnLeaveEqualToLabResourceProduction | crate::cards::Special::CultureOnRevolution(_) | crate::cards::Special::CultureOnTechDevelop(_) | crate::cards::Special::CulturePerAdditionalColony(_) | crate::cards::Special::CulturePerCivilizationWithMoreCulture(_) | crate::cards::Special::CulturePerHappyFromTemplesTheatersWonders(_) | crate::cards::Special::CulturePerLabEqualToLevel | crate::cards::Special::CulturePerLibraryTheaterPair(_) | crate::cards::Special::CulturePerTheater(_) | crate::cards::Special::DecreasePopulation(_) | crate::cards::Special::DestroyUrbanBuildings(_) | crate::cards::Special::DoubleBestMine | crate::cards::Special::DoublesTacticBonusOfOneArmy | crate::cards::Special::ExtraHappyPerHappySource(_) | crate::cards::Special::FinalScoring(_) | crate::cards::Special::FreeCivilAction(_) | crate::cards::Special::FreePopIncreasePerTurn | crate::cards::Special::Gain(_) | crate::cards::Special::GainCulturePerLevelOfRemovedCard(_) | crate::cards::Special::GainFoodOrResources(_) | crate::cards::Special::GainResources(_) | crate::cards::Special::InfantryCountsAsCavalryForTactics | crate::cards::Special::LastRoundSubstitute(_) | crate::cards::Special::LeaderTakeCivilActionDiscount(_) | crate::cards::Special::LibraryDiscountsIfTheater | crate::cards::Special::Lose(_) | crate::cards::Special::MilitaryActionAsCivilPerTurn(_) | crate::cards::Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | crate::cards::Special::NoAttacksBetweenParties | crate::cards::Special::OnAttackBetweenParties(_) | crate::cards::Special::OnBuildCulture(_) | crate::cards::Special::OnBuildCulturePerTechLevelSum | crate::cards::Special::OnReplacePutUnderCompletedWonderHappy(_) | crate::cards::Special::OncePerGameTwoPoliticalActions | crate::cards::Special::OpponentDecreasesPopulation(_) | crate::cards::Special::OpponentsPayDoubleMilitaryActionsToAttackYou | crate::cards::Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | crate::cards::Special::PeekTopEventCardInPolitics | crate::cards::Special::PerTurnChoice | crate::cards::Special::PlayerWithLeastCulture(_) | crate::cards::Special::PlayerWithMostCulture(_) | crate::cards::Special::PlayersWithMostDiscontentWorkers(_) | crate::cards::Special::PlayersWithMostHappyFaces(_) | crate::cards::Special::PopIncreaseFoodDiscount(_) | crate::cards::Special::RemoveAsPoliticalActionForYellowToken(_) | crate::cards::Special::RemoveAsPoliticalActionFreeColonize | crate::cards::Special::RemoveFromGame | crate::cards::Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | crate::cards::Special::ResourceOnTechDevelop(_) | crate::cards::Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | crate::cards::Special::ResourcesPerLabEqualToLevel | crate::cards::Special::RevolutionUsesMilitaryActionsInstead | crate::cards::Special::ScienceOnTechCardTake(_) | crate::cards::Special::SciencePerBestLabOrLibraryLevel | crate::cards::Special::SciencePerLab(_) | crate::cards::Special::StealColony(_) | crate::cards::Special::StrengthPerArtillery(_) | crate::cards::Special::StrengthPerInfantry(_) | crate::cards::Special::StrengthPerMilitaryUnit(_) | crate::cards::Special::StrengthPerTempleOrGovernmentHappy(_) | crate::cards::Special::StrengthPerUnitType(_) | crate::cards::Special::StrongestPlayer(_) | crate::cards::Special::StrongestPlayers(_) | crate::cards::Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | crate::cards::Special::TakeFromOpponent(_) | crate::cards::Special::TheaterResourceDiscountIfLibrary(_) | crate::cards::Special::TheaterScienceDiscountIfLibrary(_) | crate::cards::Special::TheaterTechScienceDiscount(_) | crate::cards::Special::VictorTakesCulture | crate::cards::Special::VictorTakesScienceUpTo(_) | crate::cards::Special::VictorTakesYellowTokens | crate::cards::Special::WeakestPlayer(_) | crate::cards::Special::WeakestPlayers(_) | crate::cards::Special::WonderTakeNoExtraCivilActions => None,
        })
        .unwrap_or(0)
}

/// The military cards James Cook may burn for +1 colonization force each.
/// Mirrors `engine/interact.py::cook_pool`.
///
/// *"When colonizing, you may discard up to 2 military cards, gaining +1
/// colony bonus for each card discarded."* ANY military card -- the card
/// says "military cards", not "bonus cards", so a spent event, a tactic or a
/// pact all qualify.
///
/// BONUS CARDS ARE EXCLUDED, and that is an ordering fact rather than a
/// rule: §11.3 already lets ANY NUMBER of bonus cards into the force at
/// their printed colonization value (1/2/3), unbounded and at least the flat
/// +1 this pays, so spending one of Cook's two slots on a card that
/// `send_bonus` pays at least as much for is weakly dominated in every
/// position.
///
/// ONE ENTRY PER CARD, like [`unit_pool`]/[`bonus_pool`] and unlike
/// [`discard_options`] (which de-duplicates for a menu): two copies of one
/// card are two discards. [`colonize_moves`] de-duplicates the MOVES, which
/// is the place that wants distinct names.
///
/// Ordered least-useful-first by [`defense_points`], for the reason written
/// on [`discard_options`]: the evaluators cannot tell two same-age military
/// cards apart, so the fallback has to be "burn the card that defends least".
pub fn cook_pool(p: &PlayerState) -> CardList<MAX_FORCE> {
    let mut out = CardList::<MAX_FORCE>::new();
    if !leader_is(p, "James Cook") {
        return out;
    }
    for &id in p.hand_military.as_slice() {
        if id.kind() != CardType::Bonus {
            out.push(id);
        }
    }
    out.as_mut_slice().sort_by_key(|id| (defense_points(*id), id.name()));
    out
}

/// Colonization force of a concrete sacrifice (§11.3, + James Cook).
pub fn force_value(
    state: &GameState,
    p: &PlayerState,
    units: &[CardId],
    bonuses: &[CardId],
    discards: &[CardId],
) -> i32 {
    if units.is_empty() {
        return 0;
    }
    let mut total: i32 = units.iter().map(|id| id.get().effects.strength as i32).sum();
    // §10.7: only the SACRIFICED units form armies.
    total += effects::army_strength_units(p, units);
    total += effects::state_stats(state, p).colonize;
    total += bonuses.iter().map(|id| id.get().effects.colonization_bonus as i32).sum::<i32>();
    total += cook_bonus_per_discard(p) * discards.len() as i32;
    total
}

/// How much force James Cook's discards could add at most (0 for anybody
/// else). Mirrors `engine/interact.py::cook_ceiling`.
fn cook_ceiling(p: &PlayerState) -> i32 {
    let pool = cook_pool(p);
    cook_bonus_per_discard(p) * (COOK_DISCARDS.min(pool.len() as i32))
}

/// Largest force this player could send -- the bidding ceiling (§11.2).
///
/// §11.2 caps a bid at "the maximum colonization force the bidder can
/// actually send", so James Cook's two discards belong in it: they are
/// force he can actually send, and leaving them out would let him win an
/// auction and then be unable to bid the amount his own card was bought for.
pub fn max_force(state: &GameState, p: &PlayerState) -> i32 {
    let units = unit_pool(p);
    if units.is_empty() {
        return 0;
    }
    let bonuses = bonus_pool(p);
    force_value(state, p, units.as_slice(), bonuses.as_slice(), &[]) + cook_ceiling(p)
}

/// A territory card revealed as the current event is auctioned (§11.1).
///
/// RULES_SPEC 11.1: the auction is "open to all players" -- every
/// non-resigned player gets a real bid/pass turn in clockwise order from the
/// revealer, INCLUDING one whose own `max_force` is currently 0. Such a
/// player has no legal `Bid` (`pending_moves`'s `Move::Bid` range is already
/// empty when `ceiling <= bid`), but they are not silently dropped from the
/// turn order the way an excluded-from-`active` player would be -- BGO logs
/// their own explicit `Pass` before moving on, confirmed against real game
/// `7523791` (Green's own colonization the round before drops their unit
/// count to a computed `max_force` of exactly 0 for the very next
/// territory reveal, yet the journal's line 200 still shows a real `"Green
/// passes"` as the auction's FIRST move, before Grey/Orange/Purple ever
/// bid). The previous version of this function filtered `active` down to
/// `max_force(...) > 0` players before the auction ever started, which
/// skipped Green's turn entirely and made Grey (not Green) the computed
/// first decider -- a `decider != expected_actor` mismatch the replayer
/// cannot resolve as a forced pass, because Grey legitimately has several
/// real bids available.
///
/// Returns `false` and files the card as a past event WITHOUT opening a
/// decision only when NO player anywhere can ever bid -- that early return
/// is a distinct case from "this one player can't bid", pinned by
/// `an_auction_nobody_can_enter_is_not_started`.
pub fn start_auction(state: &mut GameState, card: CardId, revealer: u8) -> bool {
    let (order, n) = order_from(state, revealer);
    // If NOBODY anywhere can ever bid, no auction opens at all -- pinned by
    // `an_auction_nobody_can_enter_is_not_started`, unchanged from before.
    let any_bidder = order[..n].iter().any(|&i| max_force(state, &state.players[i as usize]) > 0);
    if !any_bidder {
        state.past_events.push(card);
        return false;
    }
    let mut active = [0u8; MAX_PLAYERS];
    let mut k = 0;
    for &i in &order[..n] {
        // The revealer is unconditionally entered even at `max_force == 0`
        // as long as SOMEBODY can bid -- they are already mid-interaction
        // from revealing the event, and BGO asks them for a real bid/pass
        // regardless (real game `7523791`, journal line 200: Green's own
        // colonization the round before drops Green's `max_force` to 0,
        // yet Green still logs a real `"Green passes"` as the auction's
        // first move, ahead of the other three players' bids). A
        // NON-revealer with `max_force == 0` is instead skipped over
        // entirely, never asked at all -- BGO's own auction loop only
        // force-checks entrants OTHER than the one who triggered it (real
        // games `7521683` and `7523256`: in both, a zero-force
        // non-revealer never appears in the journal around that auction at
        // all, and the revealer's own bid wins immediately with nobody
        // else's move logged in between).
        if i == revealer || max_force(state, &state.players[i as usize]) > 0 {
            active[k] = i;
            k += 1;
        }
    }
    state.pending.push(Pending::Auction(crate::state::Auction::new(card, &active[..k])));
    true
}

fn auction_move(state: &mut GameState, mv: Move) {
    let Some(Pending::Auction(a)) = state.pending.top_mut() else {
        panic!("{mv:?} against a non-auction decision")
    };
    match mv {
        Move::Bid { n } => {
            a.bid = n;
            a.high = Some(a.player);
            a.pos = ((a.pos as usize + 1) % a.active().len()) as u8;
        }
        Move::BidPass => {
            let pos = a.pos as usize;
            a.drop_at(pos);
            if a.pos as usize >= a.active().len() {
                a.pos = 0;
            }
        }
        other @ Move::Take { .. } | other @ Move::Build { .. } | other @ Move::Develop { .. } | other @ Move::Upgrade { .. } | other @ Move::WonderStep { .. } | other @ Move::Pop | other @ Move::PopFree | other @ Move::Revolution { .. } | other @ Move::PlayLeader { .. } | other @ Move::PlayAction { .. } | other @ Move::Destroy { .. } | other @ Move::PlayTactic { .. } | other @ Move::CopyTactic { .. } | other @ Move::Aggression { .. } | other @ Move::War { .. } | other @ Move::OfferPact { .. } | other @ Move::CancelPact { .. } | other @ Move::PrepareEvent { .. } | other @ Move::RemoveLeaderYellow | other @ Move::ColumbusColonize { .. } | other @ Move::Barbarossa { .. } | other @ Move::BachTheater { .. } | other @ Move::TradeFoodAsResource | other @ Move::TradeResourceAsFood | other @ Move::Defend { .. } | other @ Move::DefendDone | other @ Move::SendUnit { .. } | other @ Move::SendBonus { .. } | other @ Move::SendDiscard { .. } | other @ Move::SendDone | other @ Move::Choose { .. } | other @ Move::Churchill { .. } | other @ Move::EndTurn | other @ Move::PolPass | other @ Move::Resign => panic!("{other:?} against an auction decision"),
    }
    // Read the settled auction back out before mutating the rest of `state`.
    let (empty, sole_winner, card, bid) = {
        let Some(Pending::Auction(a)) = state.pending.top() else { unreachable!() };
        let active = a.active();
        let sole = if active.len() == 1 && a.high == Some(active[0]) { Some(active[0]) } else { None };
        (active.is_empty(), sole, a.card, a.bid)
    };
    if empty {
        state.pending.pop();
        state.past_events.push(card);
        return;
    }
    if let Some(winner) = sole_winner {
        state.pending.pop();
        colonize(state, winner, card, bid);
        return;
    }
    let Some(Pending::Auction(a)) = state.pending.top_mut() else { unreachable!() };
    a.player = a.active()[a.pos as usize];
}

/// The auction winner sacrifices a force of at least `bid` (§11.3-11.5).
///
/// RULES_SPEC 11.3 fixes only the FLOOR on the sacrifice -- printed strength
/// of the sacrificed units (>= 1 unit mandatory, even if bonuses would cover
/// the bid), plus armies, plus "the colonization value of ANY NUMBER of
/// military bonus cards played" -- and §11.2 fixes only that the winner MUST
/// colonize. WHICH units and WHICH bonus cards make up that force is the
/// player's decision, and it is a real one: a unit is permanent strength and
/// a worker, a bonus card is a consumable also worth +2/+4/+6 defence if
/// kept, and the two are traded off every time.
///
/// Single-move prefixes are auto-resolved, exactly as `push_choice(auto=true)`
/// does for a one-option `choice`: while the force so far admits only ONE
/// legal continuation there is nothing to decide.
///
/// EACH COMMITMENT IS PAID WHEN IT IS MADE, not in a lump at `send_done`.
/// That is deliberate and it is what makes the decision answerable: a bot
/// prices a candidate move by applying it to a trial state one ply deep, so a
/// `send_unit` that only appended a name would cost NOTHING an evaluator can
/// see while `send_done` carried the entire bill -- every 1-ply bot then
/// reads "commit another unit" as free and procrastinates until the pool is
/// empty. That is not hypothetical; it is a pinned Python regression
/// (`tests/test_colony_sacrifice_choice.py`). The end state is identical
/// either way.
pub fn colonize(state: &mut GameState, player: u8, card: CardId, bid: u8) {
    let (pool, bpool, dpool) = {
        let p = &state.players[player as usize];
        (unit_pool(p), bonus_pool(p), cook_pool(p))
    };
    assert!(
        !pool.is_empty(),
        "P{player} must colonize {} with no units -- unreachable from an auction \
         (start_auction only admits max_force > 0, and §11.2 caps a bid at the \
         bidder's own max_force)",
        card.name()
    );
    state.pending.push(Pending::Colonize(Colonize {
        player,
        card,
        bid,
        units: CardList::new(),
        bonuses: CardList::new(),
        discards: CardList::new(),
        pool,
        bpool,
        dpool,
    }));
    colonize_auto(state);
}

/// Play out every step of the force that has only one legal answer.
fn colonize_auto(state: &mut GameState) {
    let depth = state.pending.len();
    while state.pending.len() == depth {
        let Some(Pending::Colonize(c)) = state.pending.top() else { return };
        let moves = colonize_moves(state, c);
        if moves.len() != 1 {
            return;
        }
        let only = moves.as_slice()[0];
        colonize_step(state, only);
    }
}

/// §11.3 choices, ordered so a `moves[0]` bot reproduces the old greedy rule.
///
/// [`unit_pool`] is sorted weakest-first and [`bonus_pool`] cheapest-first, so
/// "the mandatory unit, then bonus cards, then more units, and stop as soon as
/// the bid is met" is exactly what the deleted `_build_force` did.
///
/// James Cook's discards join the list in the same place bonus cards do --
/// after the mandatory unit and after the bonus cards, because §11.3's ">= 1
/// unit" floor is a floor on the SACRIFICE and no card of any kind can stand
/// in for it. Offered only while a discard slot remains (`COOK_DISCARDS`).
fn colonize_moves(state: &GameState, c: &Colonize) -> MoveList {
    let p = &state.players[c.player as usize];
    let mut out = MoveList::new();
    if !c.units.is_empty()
        && force_value(state, p, c.units.as_slice(), c.bonuses.as_slice(), c.discards.as_slice())
            >= c.bid as i32
    {
        out.push(Move::SendDone);
    }
    let mut ubuf = [CardId::NONE; MAX_FORCE];
    let un = distinct(c.pool.as_slice(), &mut ubuf);
    if c.units.is_empty() {
        // §11.3: at least one unit, always.
        for &id in &ubuf[..un] {
            out.push(Move::SendUnit { card: id });
        }
        return out;
    }
    let mut bbuf = [CardId::NONE; MAX_FORCE];
    let bn = distinct(c.bpool.as_slice(), &mut bbuf);
    for &id in &bbuf[..bn] {
        out.push(Move::SendBonus { card: id });
    }
    if (c.discards.len() as i32) < COOK_DISCARDS {
        let mut dbuf = [CardId::NONE; MAX_FORCE];
        let dn = distinct(c.dpool.as_slice(), &mut dbuf);
        for &id in &dbuf[..dn] {
            out.push(Move::SendDiscard { card: id });
        }
    }
    for &id in &ubuf[..un] {
        out.push(Move::SendUnit { card: id });
    }
    out
}

fn colonize_move(state: &mut GameState, mv: Move) {
    colonize_step(state, mv);
    colonize_auto(state);
}

/// One commitment, paid immediately (see [`colonize`]), or the send.
fn colonize_step(state: &mut GameState, mv: Move) {
    let (player, card) = {
        let Some(Pending::Colonize(c)) = state.pending.top_mut() else {
            panic!("{mv:?} against a non-colonize decision")
        };
        match mv {
            Move::SendUnit { card } => {
                // §11.4: the token goes to the YELLOW BANK, not to free workers.
                c.pool.remove_first(card);
                c.units.push(card);
                let player = c.player;
                let p = &mut state.players[player as usize];
                if let Some(slot) = p.techs.get_mut(card) {
                    slot.workers -= 1;
                }
                p.yellow_bank += 1;
                return;
            }
            Move::SendBonus { card } => {
                // §11.6: discarded before any territory draw.
                c.bpool.remove_first(card);
                c.bonuses.push(card);
                let player = c.player;
                state.players[player as usize].hand_military.remove_first(card);
                economy::discard_military(state, card);
                return;
            }
            Move::SendDiscard { card } => {
                // James Cook, +1 colonization force per card discarded.
                c.dpool.remove_first(card);
                c.discards.push(card);
                let player = c.player;
                state.players[player as usize].hand_military.remove_first(card);
                economy::discard_military(state, card);
                return;
            }
            Move::SendDone => (c.player, c.card),
            other @ Move::Take { .. } | other @ Move::Build { .. } | other @ Move::Develop { .. } | other @ Move::Upgrade { .. } | other @ Move::WonderStep { .. } | other @ Move::Pop | other @ Move::PopFree | other @ Move::Revolution { .. } | other @ Move::PlayLeader { .. } | other @ Move::PlayAction { .. } | other @ Move::Destroy { .. } | other @ Move::PlayTactic { .. } | other @ Move::CopyTactic { .. } | other @ Move::Aggression { .. } | other @ Move::War { .. } | other @ Move::OfferPact { .. } | other @ Move::CancelPact { .. } | other @ Move::PrepareEvent { .. } | other @ Move::RemoveLeaderYellow | other @ Move::ColumbusColonize { .. } | other @ Move::Barbarossa { .. } | other @ Move::BachTheater { .. } | other @ Move::TradeFoodAsResource | other @ Move::TradeResourceAsFood | other @ Move::Bid { .. } | other @ Move::BidPass | other @ Move::Defend { .. } | other @ Move::DefendDone | other @ Move::Choose { .. } | other @ Move::Churchill { .. } | other @ Move::EndTurn | other @ Move::PolPass | other @ Move::Resign => panic!("{other:?} against a colonize decision"),
        }
    };
    state.pending.pop();
    gain_colony(state, player, card);
}

/// Permanent effects first, then the one-time effect (§11.5).
pub fn gain_colony(state: &mut GameState, player: u8, card: CardId) {
    {
        let perm = &card.get().effects;
        let p = &mut state.players[player as usize];
        p.colonies.push(card);
        crate::apply::grant_yellow(p, perm.yellow_tokens as i32);
        p.blue_total = (p.blue_total as i32 + perm.blue_tokens as i32).max(0) as u8;
    }
    apply_immediate_effects(state, player, card);
}

/// A territory's `immediateEffects` (§11.5), which Python routes through
/// `events.apply_gains`. The vocabulary is closed (`cards::ImmediateEffects`),
/// and every key in it is a plain gain, so this is the same arithmetic
/// `apply_gains` would do -- not a second, divergent copy of a general
/// gain-block interpreter. When `events.rs` lands with a real `apply_gains`,
/// this should call it.
fn apply_immediate_effects(state: &mut GameState, player: u8, card: CardId) {
    let imm = card.get().immediate_effects;
    {
        let p = &mut state.players[player as usize];
        p.science += imm.science.max(0) as u16;
        p.culture += imm.culture.max(0) as u16;
        economy::gain_food(p, imm.food.max(0) as u16);
        economy::gain_resources(p, imm.resources.max(0) as u16);
        for _ in 0..imm.population.max(0) {
            if p.yellow_bank > 0 {
                p.yellow_bank -= 1;
                p.workers_free += 1;
            }
        }
    }
    // `_draw_military`: nothing is drawn in age IV.
    if imm.draw_military_cards > 0 && state.age_military != Age::IV {
        for _ in 0..imm.draw_military_cards {
            match economy::draw_military(state) {
                Some(c) => state.players[player as usize].hand_military.push(c),
                None => break,
            }
        }
    }
}

/// The permanent effects go away; the one-time effect is never undone.
pub fn lose_colony(p: &mut PlayerState, card: CardId) {
    if !p.colonies.remove_first(card) {
        return;
    }
    let perm = &card.get().effects;
    p.yellow_bank = (p.yellow_bank as i32 - perm.yellow_tokens as i32).max(0) as u8;
    p.blue_total = (p.blue_total as i32 - perm.blue_tokens as i32).max(0) as u8;
}

// ------------------------------------------------------- aggression defense

/// §5.4.4: the defender may play bonus cards / discard military cards.
///
/// This is where the defender's committed defense total comes from -- the one
/// thing `combat.rs` could not produce without a `pending` field. With no
/// military actions or an empty hand there is nothing to decide, so the
/// aggression resolves immediately at the defender's printed strength.
pub fn start_defense(state: &mut GameState, attacker: u8, defender: u8, card: CardId, atk: i32) {
    let (budget, dfn) = {
        let d = &state.players[defender as usize];
        let s = effects::state_stats(state, d);
        (s.military_actions, s.strength)
    };
    let ctx = Defense {
        player: defender,
        attacker,
        card,
        atk,
        dfn,
        spent: 0,
        budget: budget.max(0) as u8,
    };
    if budget <= 0 || state.players[defender as usize].hand_military.is_empty() {
        combat::finish_aggression(state, &ctx);
        return;
    }
    state.pending.push(Pending::Defense(ctx));
}

fn defense_move(state: &mut GameState, mv: Move) {
    {
        let Some(Pending::Defense(d)) = state.pending.top_mut() else {
            panic!("{mv:?} against a non-defense decision")
        };
        if let Move::Defend { card } = mv {
            let player = d.player;
            d.dfn += defense_points(card);
            d.spent += 1;
            let (spent, budget) = (d.spent, d.budget);
            state.players[player as usize].hand_military.remove_first(card);
            economy::discard_military(state, card);
            // More to spend and more to spend it on: the decision stays open.
            if spent < budget && !state.players[player as usize].hand_military.is_empty() {
                return;
            }
        }
    }
    let Some(Pending::Defense(ctx)) = state.pending.pop() else { unreachable!() };
    combat::finish_aggression(state, &ctx);
}

// ================================================================ tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{
        CardList, GainOption, GainOptions, PactList, Phase, Tableau, TechSlot, MAX_PLAYERS,
        ROW_SIZE,
    };

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    fn blank_player(idx: u8) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government: card("Despotism"),
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
            civil_actions: 4,
            military_actions: 2,
            politics_done: false,
            tactic_action_used: false,
            taken_this_turn: CardList::new(),
            ca_spent_taking: 0,
            hammurabi_used: false,
            hammurabi_replaced_this_turn: false,
            breakthrough_ma_funded: false,
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
        }
    }

    fn blank_state(num_players: u8) -> GameState {
        GameState {
            num_players,
            seed: 0,
            players: std::array::from_fn(|i| blank_player(i as u8)),
            current: 0,
            turn: 1,
            round: 1,
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
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
            available_tactics: CardList::new(),
            civil_discard: std::array::from_fn(|_| CardList::new()),
            civil_removed: std::array::from_fn(|_| CardList::new()),
            discarded_military: std::array::from_fn(|_| CardList::new()),
            last_round: false,
            final_round_end: None,
            game_over: false,
            phase: Phase::Actions,
            forced_winner: None,
            pending: crate::state::PendingStack::new(),
            queue: crate::state::Queue::new(),
            last_end_of_turn_culture: [None; crate::state::MAX_PLAYERS],
            last_end_of_turn_science: [None; MAX_PLAYERS],
            last_end_of_turn_resources: [None; MAX_PLAYERS],
        }
    }

    // ------------------------------------------------------------ plumbing

    /// `push_choice`'s two auto cases are the reason no caller special-cases
    /// "nothing to choose from": zero options is a no-op, one option resolves
    /// without ever opening a decision.
    #[test]
    fn push_choice_auto_resolves_a_single_option_and_ignores_none() {
        let mut state = blank_state(2);
        assert!(!push_choice(&mut state, 0, ChoiceKind::LosePop, OptionList::new(), true));
        assert!(state.pending.is_empty());

        state.players[0].techs.insert(card("Agriculture"), TechSlot { workers: 2, stored: 0 });
        let opts = card_options(&[card("Agriculture")]);
        assert!(!push_choice(&mut state, 0, ChoiceKind::LosePop, opts, true));
        assert!(state.pending.is_empty(), "a single option must not open a decision");
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 1);
        assert_eq!(state.players[0].yellow_bank, 1);
    }

    #[test]
    fn push_choice_without_auto_opens_a_decision_even_for_one_option() {
        let mut state = blank_state(2);
        let opts = card_options(&[card("Agriculture")]);
        assert!(push_choice(&mut state, 1, ChoiceKind::DestroyOwn, opts, false));
        assert_eq!(state.decider(), 1, "the decider is the chooser, not `current`");
        assert_eq!(pending_moves(&state).as_slice(), &[Move::Choose { n: 0 }]);
    }

    /// The whole point of the module: `decider` is not `current`.
    #[test]
    fn a_pact_offer_hands_the_move_to_the_partner() {
        let mut state = blank_state(2);
        let peace = card("Peace Treaty");
        let mut opts = OptionList::new();
        opts.push(ChoiceOption::Word(Keyword::Accept));
        opts.push(ChoiceOption::Word(Keyword::Refuse));
        push_choice(
            &mut state,
            1,
            ChoiceKind::PactOffer { owner: 0, card: peace, a: 0, b: 1 },
            opts,
            false,
        );
        assert_eq!(state.current, 0);
        assert_eq!(state.decider(), 1);
        apply_pending(&mut state, Move::Choose { n: 0 });
        assert!(state.pending.is_empty());
        let pacts = state.players[0].pacts.as_slice();
        assert_eq!(pacts.len(), 1);
        assert_eq!(pacts[0].card, peace);
        assert_eq!(pacts[0].owner, 0);
        assert_eq!(pacts[0].partner, 1);
    }

    /// RULES_SPEC §5.9: "Partner accepts -> pact enters your play area ...
    /// it applies immediately." Open Borders Agreement grants BOTH parties
    /// "one military action" -- traced from real BGO game `7523341` (`docs/
    /// REPLAY.md`): Grey accepted this pact mid-Orange's-turn, well before
    /// Grey's own turn began, yet Grey's end-of-turn draw still undercounted
    /// by exactly 1 military card, because `p.military_actions` (the
    /// decrementing-only per-turn pool `economy::draw_military_step` reads)
    /// was never topped up to match what `effects::state_stats` already
    /// included the instant the pact became active -- only a `SIMULATOR`
    /// bug (`replay_common.rs`'s military-hand ledger agreed with the
    /// journal throughout), but the SAME stale-pool mechanism would make the
    /// real ENGINE miscompute a human's own legal military actions for the
    /// rest of that turn too. Both the offerer (`a`) and the accepter (`b`)
    /// must see the bump, not just whichever side happens to be `state.
    /// current` right now.
    #[test]
    fn accepting_a_pact_that_grants_military_actions_tops_up_both_parties_live_pool_immediately() {
        let mut state = blank_state(2);
        state.players[0].military_actions = 1; // offerer: 1 already spent this turn
        state.players[1].military_actions = 0; // accepter: fully spent this turn
        let open_borders = card("Open Borders Agreement");
        let mut opts = OptionList::new();
        opts.push(ChoiceOption::Word(Keyword::Accept));
        opts.push(ChoiceOption::Word(Keyword::Refuse));
        push_choice(
            &mut state,
            1,
            ChoiceKind::PactOffer { owner: 0, card: open_borders, a: 0, b: 1 },
            opts,
            false,
        );
        apply_pending(&mut state, Move::Choose { n: 0 });
        assert_eq!(
            state.players[0].military_actions, 2,
            "offerer's live military-action pool must gain the pact's +1 THIS turn"
        );
        assert_eq!(
            state.players[1].military_actions, 1,
            "accepter's live military-action pool must gain the pact's +1 THIS turn too"
        );
    }

    #[test]
    fn refusing_a_pact_returns_the_card_to_the_offerer() {
        let mut state = blank_state(2);
        let peace = card("Peace Treaty");
        let mut opts = OptionList::new();
        opts.push(ChoiceOption::Word(Keyword::Accept));
        opts.push(ChoiceOption::Word(Keyword::Refuse));
        push_choice(
            &mut state,
            1,
            ChoiceKind::PactOffer { owner: 0, card: peace, a: 0, b: 1 },
            opts,
            false,
        );
        apply_pending(&mut state, Move::Choose { n: 1 });
        assert!(state.players[0].pacts.is_empty());
        assert_eq!(state.players[0].hand_military.as_slice(), &[peace]);
    }

    // ---------------------------------------------------------- the queue

    /// `run_queue` drains only while nothing is pending, so a queued item
    /// that opens a decision suspends the rest of the queue behind it.
    #[test]
    fn a_queued_decision_suspends_the_rest_of_the_queue() {
        let mut state = blank_state(2);
        state.players[0].techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        state.players[0].techs.insert(card("Bronze"), TechSlot { workers: 1, stored: 0 });
        enqueue(&mut state, QueueItem::DestroyOwn { player: 0 });
        enqueue(&mut state, QueueItem::LoseColony { player: 0 });
        run_queue(&mut state);
        assert_eq!(state.pending.len(), 1, "destroy_own has two options, so it opens");
        assert_eq!(state.queue.len(), 1, "lose_colony must still be waiting");
    }

    /// `_q_lose_pop` re-queues its REMAINDER at the FRONT, so the rest of a
    /// multi-population loss lands before anything queued behind it.
    #[test]
    fn lose_pop_requeues_its_remainder_at_the_front() {
        let mut state = blank_state(2);
        state.players[0].techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        state.players[0].techs.insert(card("Bronze"), TechSlot { workers: 1, stored: 0 });
        enqueue(&mut state, QueueItem::LosePop { player: 0, n: 2 });
        enqueue(&mut state, QueueItem::DestroyOwn { player: 0 });
        run_queue(&mut state);
        assert_eq!(state.pending.len(), 1);
        assert_eq!(
            state.queue.iter().next(),
            Some(QueueItem::LosePop { player: 0, n: 1 }),
            "the remainder must be ahead of the destroy_own queued after it"
        );
    }

    /// Free workers are spent before anybody is asked to choose (FAQ p.15).
    #[test]
    fn lose_pop_takes_free_workers_without_a_decision() {
        let mut state = blank_state(2);
        state.players[0].workers_free = 2;
        state.players[0].techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        state.players[0].techs.insert(card("Bronze"), TechSlot { workers: 1, stored: 0 });
        enqueue(&mut state, QueueItem::LosePop { player: 0, n: 2 });
        run_queue(&mut state);
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].workers_free, 0);
        assert_eq!(state.players[0].yellow_bank, 2);
    }

    /// A resigned player's deferred sub-effects never happen (§5.11).
    #[test]
    fn run_queue_skips_a_resigned_players_items() {
        let mut state = blank_state(2);
        state.players[0].resigned = true;
        state.players[0].techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        state.players[0].techs.insert(card("Bronze"), TechSlot { workers: 1, stored: 0 });
        enqueue(&mut state, QueueItem::DestroyOwn { player: 0 });
        run_queue(&mut state);
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 1);
    }

    #[test]
    fn a_gain_block_choice_pays_the_branch_that_was_picked() {
        let mut state = blank_state(2);
        state.players[0].blue_total = 20;
        let mut opts = GainOptions::new();
        opts.push(GainOption { food: 2, resources: 0 });
        opts.push(GainOption { food: 0, resources: 2 });
        enqueue(&mut state, QueueItem::Choose { player: 0, options: opts });
        run_queue(&mut state);
        assert_eq!(state.pending.len(), 1);
        apply_pending(&mut state, Move::Choose { n: 1 });
        assert_eq!(state.players[0].food, 0);
        assert_eq!(state.players[0].resources, 2);
    }

    // ------------------------------------------------------------ discards

    /// The presentation order is derived from the engine's own defense
    /// arithmetic, not from the alphabet: a Military Bonus worth 6 on defense
    /// must be the LAST thing a `moves[0]` bot throws away.
    #[test]
    fn discard_options_puts_the_least_useful_defender_first() {
        let mut hand = Vec::new();
        for name in ["Military Bonus (defense 6 / colonization 3)", "Rebellion"] {
            hand.push(card(name));
        }
        let mut buf = [CardId::NONE; MAX_HAND];
        let n = discard_options(&hand, &mut buf);
        assert_eq!(n, 2);
        assert_eq!(buf[0], card("Rebellion"), "a plain card defends 1 and goes first");
        assert_eq!(defense_points(buf[0]), 1);
        assert_eq!(defense_points(buf[1]), 6);
    }

    #[test]
    fn discard_options_deduplicates() {
        let c = card("Rebellion");
        let mut buf = [CardId::NONE; MAX_HAND];
        assert_eq!(discard_options(&[c, c, c], &mut buf), 1);
    }

    // ----------------------------------------------------------- war spoils

    /// FAQ p.8: a technology already in the victor's play area or hand is not
    /// on offer, and neither is one the remaining advantage cannot pay for.
    #[test]
    fn war_tech_options_excludes_owned_held_and_unaffordable() {
        let mut state = blank_state(2);
        let masonry = card("Masonry"); // techCost 3
        let cartography = card("Cartography"); // techCost 4
        let code = card("Code of Laws"); // techCost 6
        state.players[1].techs.insert(masonry, TechSlot::default());
        state.players[1].techs.insert(cartography, TechSlot::default());
        state.players[1].techs.insert(code, TechSlot::default());
        state.players[0].techs.insert(masonry, TechSlot::default()); // already in play
        state.players[0].hand_civil.push(cartography); // already in hand
        let opts = war_tech_options(&state, 0, 1, 6);
        assert_eq!(opts.as_slice(), &[ChoiceOption::Card(code)]);
        // ...and with a budget of 5, Code of Laws is out of reach too.
        assert!(war_tech_options(&state, 0, 1, 5).is_empty());
    }

    /// `WAR_TECH_SCIENCE_IDX` is a promise about option 0.
    #[test]
    fn war_tech_science_is_always_option_zero() {
        let mut state = blank_state(2);
        state.players[1].techs.insert(card("Masonry"), TechSlot::default());
        state.players[1].science = 10;
        war_tech_spoils(&mut state, 0, 1, 6);
        let Some(Pending::Choice(c)) = state.pending.top() else { panic!("no decision opened") };
        assert_eq!(
            c.options.get(WAR_TECH_SCIENCE_IDX as usize),
            Some(ChoiceOption::Word(Keyword::Science))
        );
    }

    /// `settle_war_spoils` is the LOOKAHEAD-only stand-in for the victor's
    /// real choice (see its doc comment): with a stealable technology on
    /// offer it must take that steal -- the ceiling `war_tech_options` can
    /// offer for the budget -- and only fall back to science for whatever
    /// budget is left over, not hardcode science for the whole advantage.
    ///
    /// This is the regression test for the one-sided bias described on
    /// `settle_war_spoils`: before the fix it always chose
    /// `WAR_TECH_SCIENCE_IDX` and this player would have ended with 6
    /// science and Masonry untouched.
    #[test]
    fn settle_war_spoils_prefers_stealing_a_technology_over_taking_science() {
        let mut state = blank_state(2);
        let masonry = card("Masonry"); // techCost 3
        state.players[1].techs.insert(masonry, TechSlot::default());
        state.players[1].science = 10;
        war_tech_spoils(&mut state, 0, 1, 6);
        settle_war_spoils(&mut state);
        assert!(state.pending.is_empty());
        assert!(state.players[0].techs.has(masonry), "the steal must be taken, not skipped");
        assert!(!state.players[1].techs.has(masonry));
        // Masonry costs 3 of the 6-point advantage; the remaining 3 has
        // nothing left to steal, so it lands as science.
        assert_eq!(state.players[0].science, 3);
        assert_eq!(state.players[1].science, 7);
    }

    /// FAQ p.8's cap: the victor cannot take more science than the loser has.
    #[test]
    fn take_war_science_is_capped_at_the_losers_stock() {
        let mut state = blank_state(2);
        state.players[1].science = 2;
        take_war_science(&mut state, 0, 1, 9);
        assert_eq!(state.players[0].science, 2);
        assert_eq!(state.players[1].science, 0);
    }

    /// Mixing is legal, so a steal re-offers with the advantage reduced by
    /// the card's PRINTED cost and the remainder is still takeable.
    #[test]
    fn stealing_a_technology_reoffers_with_the_reduced_budget() {
        let mut state = blank_state(2);
        let masonry = card("Masonry"); // techCost 3
        state.players[1].techs.insert(masonry, TechSlot::default());
        state.players[1].science = 10;
        war_tech_spoils(&mut state, 0, 1, 8);
        // Option 0 is science, option 1 is Masonry.
        apply_pending(&mut state, Move::Choose { n: 1 });
        assert!(state.players[0].techs.has(masonry), "the card changed play areas");
        assert!(!state.players[1].techs.has(masonry));
        // Nothing left to steal, so the remaining 5 is taken as science with
        // no second decision.
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].science, 5);
        assert_eq!(state.players[1].science, 5);
    }

    /// No stealable technology at all means there is no decision to make --
    /// the science arm resolves straight through.
    #[test]
    fn war_tech_with_nothing_stealable_makes_no_decision() {
        let mut state = blank_state(2);
        state.players[1].science = 4;
        war_tech_spoils(&mut state, 0, 1, 7);
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].science, 4);
    }

    // -------------------------------------------------------------- plunder

    /// FAQ p.7: the split between food and resources is the ATTACKER's
    /// choice, not a fixed "resources first" order. With the defender
    /// holding both banks, "take up to 3" has three genuine ways to reach
    /// the full 3 (3 resources, 2+1, or 1+2), and this pins that all three
    /// are actually offered -- before this fix the engine only ever
    /// produced the first of them.
    #[test]
    fn plunder_offers_the_attacker_a_real_split_choice_instead_of_resources_first() {
        let mut state = blank_state(2);
        state.players[1].food = 2;
        state.players[1].resources = 5;
        // Headroom so the attacker's own blue-token bank is not itself the
        // bottleneck -- this test is about the SPLIT, not the gain cap.
        state.players[0].blue_total = 20;
        start_defense(&mut state, 0, 1, card("Aggression: Plunder (I)"), 5);
        let Some(Pending::Choice(c)) = state.pending.top() else { panic!("no decision opened") };
        assert!(matches!(c.kind, ChoiceKind::PlunderSplit { victim: 1 }));
        assert_eq!(
            c.options.as_slice(),
            &[
                ChoiceOption::Gain(GainOption { food: 0, resources: 3 }),
                ChoiceOption::Gain(GainOption { food: 1, resources: 2 }),
                ChoiceOption::Gain(GainOption { food: 2, resources: 1 }),
            ]
        );
        // Choosing the food-heavy split must actually move FOOD, not
        // resources -- pinning that the mix is a real decision rather than
        // window dressing over the old hardcoded order.
        apply_pending(&mut state, Move::Choose { n: 2 });
        assert!(state.pending.is_empty());
        assert_eq!(state.players[1].food, 0);
        assert_eq!(state.players[1].resources, 4);
        assert_eq!(state.players[0].food, 2);
        assert_eq!(state.players[0].resources, 1);
    }

    /// A single feasible split (the defender holds only one of the two
    /// banks) is not a real decision, so it auto-resolves without opening
    /// one -- `push_choice`'s own `auto` contract.
    #[test]
    fn plunder_with_only_one_bank_available_auto_resolves() {
        let mut state = blank_state(2);
        state.players[1].resources = 5;
        state.players[1].food = 0;
        state.players[0].blue_total = 20;
        start_defense(&mut state, 0, 1, card("Aggression: Plunder (I)"), 5);
        assert!(state.pending.is_empty(), "only one split reaches the cap -- no decision to open");
        assert_eq!(state.players[1].resources, 2);
        assert_eq!(state.players[0].resources, 3);
    }

    // ------------------------------------------------------------- defense

    /// With no military actions there is nothing to decide, so the aggression
    /// resolves at once -- and at the defender's printed strength.
    /// An empty military hand leaves nothing to decide, so §5.4.4 is skipped
    /// and the aggression resolves at the defender's PRINTED strength.
    ///
    /// Note what `budget` is: `effects::state_stats(defender).military_actions`
    /// -- the government's printed rate, NOT `p.military_actions` (what is
    /// left this turn). That is Python's reading too, and it is easy to get
    /// backwards, so it is pinned here.
    #[test]
    fn start_defense_with_an_empty_hand_resolves_immediately() {
        let mut state = blank_state(2);
        assert!(state.players[1].hand_military.is_empty());
        // Defender strength 0 vs attacker 0: `dfn >= atk`, so the aggression
        // FAILS and resolves completely.
        start_defense(&mut state, 0, 1, card("Aggression: Raid (I)"), 0);
        assert!(state.pending.is_empty());
    }

    /// ...and a SUCCEEDING aggression resolves fully now that `combat::
    /// finish_aggression`'s success branch is ported (2026-08-05). The
    /// defender here has an empty tableau, so "Aggression: Raid (I)"'s
    /// `destroyUrbanBuildings` raid has nothing to destroy -- `run_item`
    /// isn't called by `start_defense` itself (that is `interact::run_queue`,
    /// driven by `apply`'s tail), so the raid is left sitting in the queue,
    /// unresolved but present, which is exactly what this pins.
    #[test]
    fn a_successful_aggression_resolves_its_success_effects() {
        let mut state = blank_state(2);
        start_defense(&mut state, 0, 1, card("Aggression: Raid (I)"), 5);
        assert!(state.pending.is_empty(), "nothing to destroy -- no decision opened");
        assert_eq!(
            state.queue.iter().collect::<Vec<_>>(),
            vec![QueueItem::Raid { player: 0, victim: 1, max_age: Age::I, no_loot: false }]
        );
    }

    /// A failed aggression is fully resolvable, and it is the branch the
    /// defense decision exists to reach.
    #[test]
    fn a_defense_that_holds_resolves_the_aggression() {
        let mut state = blank_state(2);
        let bonus = card("Military Bonus (defense 6 / colonization 3)");
        state.players[1].military_actions = 3;
        state.players[1].hand_military.push(bonus);
        start_defense(&mut state, 0, 1, card("Aggression: Raid (I)"), 4);
        assert_eq!(state.decider(), 1, "the DEFENDER answers, not the attacker");
        assert_eq!(
            pending_moves(&state).as_slice(),
            &[Move::DefendDone, Move::Defend { card: bonus }]
        );
        apply_pending(&mut state, Move::Defend { card: bonus });
        // 0 printed + 6 from the bonus >= 4: the aggression fails, and the
        // whole decision is gone.
        assert!(state.pending.is_empty());
        assert!(state.players[1].hand_military.is_empty());
    }

    // -------------------------------------------------------- colonization

    /// §11.3's floor: at least one unit, always -- bonus cards are not on
    /// offer until a unit has been committed.
    #[test]
    fn a_colonization_force_must_start_with_a_unit() {
        let mut state = blank_state(2);
        let warriors = card("Warriors");
        let bonus = card("Military Bonus (defense 2 / colonization 1)");
        state.players[0].techs.insert(warriors, TechSlot { workers: 2, stored: 0 });
        state.players[0].hand_military.push(bonus);
        let terr = card("Developed Territory (I)");
        colonize(&mut state, 0, terr, 3);
        // The first unit was FORCED (bonus cards are not on offer until a
        // unit is committed, and there is only one unit type), so `colonize`
        // auto-played it rather than asking. Only now, with the §11.3 floor
        // met, does the bonus card become a legal continuation.
        let Some(Pending::Colonize(c)) = state.pending.top() else {
            panic!("no colonize decision")
        };
        assert_eq!(c.units.as_slice(), &[warriors]);
        assert_eq!(
            pending_moves(&state).as_slice(),
            &[Move::SendBonus { card: bonus }, Move::SendUnit { card: warriors }],
        );
    }

    /// The §11.3 floor itself, read straight off the move generator with an
    /// empty force: no `send_bonus` and no `send_done` until a unit is in.
    #[test]
    fn an_empty_force_offers_only_units() {
        let mut state = blank_state(2);
        let warriors = card("Warriors");
        state.players[0].techs.insert(warriors, TechSlot { workers: 2, stored: 0 });
        state.players[0].hand_military.push(card("Military Bonus (defense 2 / colonization 1)"));
        let p = &state.players[0];
        let c = Colonize {
            player: 0,
            card: card("Developed Territory (I)"),
            bid: 0,
            units: CardList::new(),
            bonuses: CardList::new(),
            discards: CardList::new(),
            pool: unit_pool(p),
            bpool: bonus_pool(p),
            dpool: cook_pool(p),
        };
        assert_eq!(colonize_moves(&state, &c).as_slice(), &[Move::SendUnit { card: warriors }]);
    }

    /// Each commitment is paid the moment it is made (see `colonize`).
    #[test]
    fn committing_a_unit_pays_for_it_immediately() {
        let mut state = blank_state(2);
        let warriors = card("Warriors");
        state.players[0].techs.insert(warriors, TechSlot { workers: 2, stored: 0 });
        state.players[0].hand_military.push(card("Military Bonus (defense 2 / colonization 1)"));
        colonize(&mut state, 0, card("Developed Territory (I)"), 3);
        // The forced first unit is already committed -- and already PAID for,
        // which is the whole point: a `send_unit` that only appended a name
        // would cost nothing an evaluator can see one ply deep.
        assert_eq!(
            state.players[0].techs.workers(warriors),
            1,
            "the worker left the card as soon as it was committed"
        );
        assert_eq!(state.players[0].yellow_bank, 1, "§11.4: to the yellow bank");
        assert_eq!(state.players[0].workers_free, 0, "NOT to the free-worker pool");
        // The colony is not gained until `send_done`, so nothing else has
        // been paid out yet.
        assert!(state.players[0].colonies.is_empty());
    }

    // ------------------------------------------------------- James Cook

    #[test]
    fn cook_pool_excludes_bonus_cards_and_orders_by_defense_then_name() {
        let mut state = blank_state(2);
        state.players[0].leader = card("James Cook");
        state.players[0].hand_military.push(card("Military Bonus (defense 2 / colonization 1)"));
        state.players[0].hand_military.push(card("War over Territory"));
        state.players[0].hand_military.push(card("Aggression: Raid (I)"));
        let pool = cook_pool(&state.players[0]);
        // The bonus card is excluded (`send_bonus` already pays at least as
        // much, uncapped -- see `cook_pool`'s doc comment); every other
        // military card defends at the flat +1 of a face-down card, so the
        // tie is broken by name.
        assert_eq!(pool.as_slice(), &[card("Aggression: Raid (I)"), card("War over Territory")]);
    }

    #[test]
    fn cook_pool_is_empty_for_any_other_leader() {
        let mut state = blank_state(2);
        state.players[0].hand_military.push(card("War over Territory"));
        assert!(cook_pool(&state.players[0]).is_empty());
    }

    /// §11.2 caps a bid at the force the bidder can ACTUALLY send, and James
    /// Cook's discards are force he can actually send -- leaving them out of
    /// `max_force` would let him win an auction and then be unable to bid
    /// the amount his own card was bought for.
    #[test]
    fn max_force_includes_cooks_discard_ceiling() {
        let mut state = blank_state(2);
        let warriors = card("Warriors"); // strength 1, no other force source
        state.players[0].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        let without_cook = max_force(&state, &state.players[0]);

        state.players[0].leader = card("James Cook");
        state.players[0].hand_military.push(card("Aggression: Raid (I)"));
        state.players[0].hand_military.push(card("War over Territory"));
        assert_eq!(
            max_force(&state, &state.players[0]),
            without_cook + 2,
            "both discard slots, at +1 force each"
        );
    }

    /// James Cook, *"when colonizing, you may discard up to 2 military
    /// cards, gaining +1 colony bonus for each card discarded"* -- exercised
    /// end to end through the public `colonize`/`apply_pending` entry
    /// points, the same way `an_auction_ends_when_one_bidder_is_left_
    /// holding_the_high_bid` exercises an auction.
    #[test]
    fn james_cook_may_discard_a_military_card_for_plus_one_colonization_force() {
        let mut state = blank_state(2);
        state.players[0].leader = card("James Cook");
        let warriors = card("Warriors"); // strength 1
        state.players[0].techs.insert(warriors, TechSlot { workers: 2, stored: 0 });
        let raid = card("Aggression: Raid (I)");
        let war = card("War over Territory");
        state.players[0].hand_military.push(raid);
        state.players[0].hand_military.push(war);
        let terr = card("Developed Territory (I)");
        colonize(&mut state, 0, terr, 2); // one Warriors alone (force 1) is short of a bid of 2
        assert_eq!(state.decider(), 0, "the first unit was forced in, but the force is still short");
        let moves = pending_moves(&state);
        assert!(moves.as_slice().contains(&Move::SendDiscard { card: raid }));
        assert!(moves.as_slice().contains(&Move::SendDiscard { card: war }));
        assert!(!moves.as_slice().contains(&Move::SendDone), "1 strength does not meet a bid of 2 yet");

        apply_pending(&mut state, Move::SendDiscard { card: raid });
        assert_eq!(
            state.players[0].hand_military.as_slice(),
            &[war],
            "the discarded card left the hand immediately"
        );
        // +1 force from the discard now meets the bid of 2 -- but a second
        // Warriors and a second discard are still legal, so `colonize_auto`
        // must not have resolved the decision on its own.
        assert!(
            pending_moves(&state).as_slice().contains(&Move::SendDone),
            "the discard's +1 force now meets the bid"
        );

        apply_pending(&mut state, Move::SendDone);
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].colonies.as_slice(), &[terr]);
        assert_eq!(
            state.discarded_military[Age::I as usize].as_slice(),
            &[raid],
            "the discard goes to the normal military discard pile, filed by its own age"
        );
    }

    /// A one-unit force that already meets the bid has exactly one legal
    /// continuation, so `colonize` auto-plays it and never opens a decision.
    #[test]
    fn a_forced_colonization_never_reaches_a_bot() {
        let mut state = blank_state(2);
        let warriors = card("Warriors"); // strength 1
        state.players[0].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        let terr = card("Developed Territory (I)");
        colonize(&mut state, 0, terr, 1);
        // send_unit was the only move, then send_done was the only move.
        assert!(state.pending.is_empty(), "nothing was ever left to decide");
        assert_eq!(state.players[0].colonies.as_slice(), &[terr]);
        assert_eq!(state.players[0].techs.workers(warriors), 0);
    }

    /// RULES_SPEC 11.1: the auction is "open to all players", so a player
    /// with zero `max_force` (no units to sacrifice at all) is still owed a
    /// real turn in the clockwise order -- their only legal move is
    /// `BidPass`, but they are not skipped over the way `start_auction`
    /// used to skip any player failing its own `max_force(...) > 0` filter.
    /// Pinned against real game `7523791`: Green's own colonization the
    /// round before leaves Green's computed `max_force` at exactly 0 for
    /// the very next territory reveal, yet journal line 200 still shows a
    /// real `"Green passes"` as that auction's FIRST move, ahead of
    /// Grey/Orange/Purple's bids -- proving BGO asked Green, it did not
    /// silently route around them.
    #[test]
    fn a_zero_force_player_still_gets_a_real_turn_in_the_auction() {
        let mut state = blank_state(3);
        let warriors = card("Warriors");
        // Players 1 and 2 have force; player 0 (the revealer) has none.
        state.players[1].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        state.players[2].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        let terr = card("Developed Territory (I)");
        assert!(start_auction(&mut state, terr, 0), "players 1 and 2 can still bid");
        assert_eq!(
            state.decider(),
            0,
            "the zero-force revealer is still first in turn order, not skipped"
        );
        assert_eq!(
            pending_moves(&state).as_slice(),
            &[Move::BidPass],
            "a zero-force player's only legal move is to pass, but the auction still owes them one"
        );
        apply_pending(&mut state, Move::BidPass);
        assert_eq!(state.decider(), 1, "after passing, the turn moves to the next real bidder");
    }

    /// The zero-force exemption above is REVEALER-only: a non-revealer with
    /// no units to sacrifice is skipped over entirely, never asked at all.
    /// Pinned against real games `7521683` and `7523256`: in both, a
    /// zero-force non-revealer never appears in the journal around the
    /// auction, and the revealer's own bid wins immediately with nobody
    /// else's move logged in between -- so unlike the revealer, a
    /// non-revealer at 0 force must stay OUT of the active list, or the
    /// replayer would wait forever for a `Pass` BGO never logs.
    #[test]
    fn a_zero_force_non_revealer_is_skipped_entirely() {
        let mut state = blank_state(3);
        let warriors = card("Warriors");
        // Player 0 (the revealer) has force; player 1 has none and must be
        // skipped; player 2 has force and stays in the turn order.
        state.players[0].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        state.players[2].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        let terr = card("Developed Territory (I)");
        assert!(start_auction(&mut state, terr, 0));
        assert_eq!(state.decider(), 0, "the revealer (who has force) goes first, as usual");
        apply_pending(&mut state, Move::Bid { n: 1 });
        assert_eq!(
            state.decider(),
            2,
            "player 1 (zero force) is skipped over entirely, straight to player 2"
        );
    }

    /// Nobody with a unit means no auction at all -- the territory is filed
    /// as a past event instead of hanging on a decision with no bidders.
    #[test]
    fn an_auction_nobody_can_enter_is_not_started() {
        let mut state = blank_state(3);
        let terr = card("Developed Territory (I)");
        assert!(!start_auction(&mut state, terr, 0));
        assert!(state.pending.is_empty());
        assert_eq!(state.past_events.as_slice(), &[terr]);
    }

    /// A pass REMOVES a bidder and the survivors keep their clockwise order;
    /// the last one standing with the high bid wins without another move.
    #[test]
    fn an_auction_ends_when_one_bidder_is_left_holding_the_high_bid() {
        let mut state = blank_state(2);
        let warriors = card("Warriors");
        for i in 0..2 {
            state.players[i].techs.insert(warriors, TechSlot { workers: 2, stored: 0 });
        }
        let terr = card("Developed Territory (I)");
        assert!(start_auction(&mut state, terr, 0));
        assert_eq!(state.decider(), 0);
        apply_pending(&mut state, Move::Bid { n: 1 });
        assert_eq!(state.decider(), 1, "the auction moves clockwise");
        apply_pending(&mut state, Move::BidPass);
        // P0 won at 1 and now owes a force of at least 1. One Warriors is
        // forced in (§11.3) and meets the bid, but a SECOND Warriors is still
        // committable, so there is a real "stop here?" decision left.
        assert_eq!(state.decider(), 0);
        assert_eq!(
            pending_moves(&state).as_slice(),
            &[Move::SendDone, Move::SendUnit { card: warriors }]
        );
        apply_pending(&mut state, Move::SendDone);
        assert_eq!(state.players[0].colonies.as_slice(), &[terr]);
        assert!(state.pending.is_empty());
    }

    /// Everybody passing leaves the territory unclaimed, not a hung decision.
    #[test]
    fn an_auction_where_everybody_passes_files_the_territory() {
        let mut state = blank_state(2);
        let warriors = card("Warriors");
        for i in 0..2 {
            state.players[i].techs.insert(warriors, TechSlot { workers: 1, stored: 0 });
        }
        let terr = card("Developed Territory (I)");
        start_auction(&mut state, terr, 0);
        apply_pending(&mut state, Move::BidPass);
        apply_pending(&mut state, Move::BidPass);
        assert!(state.pending.is_empty());
        assert_eq!(state.past_events.as_slice(), &[terr]);
        assert!(state.players[0].colonies.is_empty());
    }

    /// §11.5's order: the permanent bonus lands, then the one-time effect.
    #[test]
    fn gaining_a_colony_pays_permanent_then_immediate_effects() {
        let mut state = blank_state(2);
        let terr = card("Developed Territory (I)");
        gain_colony(&mut state, 0, terr);
        assert_eq!(state.players[0].colonies.as_slice(), &[terr]);
        let perm = &terr.get().effects;
        assert_eq!(state.players[0].yellow_bank as i32, perm.yellow_tokens.max(0) as i32);
    }

    /// Losing a colony undoes the permanent bonus and nothing else.
    #[test]
    fn losing_a_colony_undoes_only_the_permanent_half() {
        let mut state = blank_state(2);
        let terr = card("Developed Territory (I)");
        gain_colony(&mut state, 0, terr);
        let science_after_gain = state.players[0].science;
        lose_colony(&mut state.players[0], terr);
        assert!(state.players[0].colonies.is_empty());
        assert_eq!(state.players[0].yellow_bank, 0);
        assert_eq!(
            state.players[0].science, science_after_gain,
            "the one-time effect is never undone"
        );
    }

    /// A single player can win every colonization auction in the game without
    /// `colonies` overflowing. §11 has no rule capping how many colonies one
    /// player may hold, and `card_table.rs` prints exactly 12 territory cards
    /// for the whole game (6 Age I + 6 Age II, `count: [1, 1, 1]` each) --
    /// nobody else ever needs to contest an auction for one player to end up
    /// holding all 12. This is not hypothetical: production's trained 4p
    /// champion hits exactly this (`gain_colony` pushing a 9th colony into a
    /// `CardList<8>`, `debug_assert!` message "CardList<8> overflow",
    /// 2026-08-06) because its policy rarely contests colonization. Before
    /// the `colonies: CardList<8>` -> `CardList<12>` fix this panics on the
    /// 9th push; confirmed by reverting the field and re-running.
    #[test]
    fn one_player_can_win_every_colonization_auction_in_the_game() {
        let all_territories = [
            "Vast Territory (I)",
            "Wealthy Territory (I)",
            "Inhabited Territory (I)",
            "Strategic Territory (I)",
            "Historic Territory (I)",
            "Developed Territory (I)",
            "Vast Territory (II)",
            "Wealthy Territory (II)",
            "Inhabited Territory (II)",
            "Strategic Territory (II)",
            "Historic Territory (II)",
            "Developed Territory (II)",
        ];
        assert_eq!(all_territories.len(), 12, "every territory card in the base game");
        let mut state = blank_state(2);
        for name in all_territories {
            gain_colony(&mut state, 0, card(name));
        }
        assert_eq!(state.players[0].colonies.len(), 12);
    }

    // ------------------------------------------------- Raiders / Foray split
    //
    // FOODFIX: §5.3's `foodAndOrResources` gain/lose block used to be a
    // fixed "resources first" formula (`events::food_or_resources`,
    // deleted); it is now the targeted player's own choice, resolved
    // through `QueueItem::FoodOrResSplit` / `ChoiceKind::FoodOrResSplit`
    // exactly like Plunder's already-real `PlunderSplit`. `events.rs` covers
    // the enqueue shape and clockwise multi-player ordering; these cover
    // option enumeration (including a player who cannot pay one pool alone)
    // and resolution, both directions.

    /// Foray's gain side: every split of the total, with NO cap at
    /// option-build time (blue tokens cap what actually lands at
    /// resolution instead -- see `food_or_res_gain_options`'s own doc).
    /// Ordered highest-resources-first, matching `plunder_split_options`'s
    /// own convention (index 0 is the old fixed-formula default).
    #[test]
    fn food_or_res_gain_options_offers_every_split_with_no_pool_cap() {
        let opts = food_or_res_gain_options(3);
        assert_eq!(
            opts.as_slice(),
            &[
                ChoiceOption::Gain(GainOption { food: 0, resources: 3 }),
                ChoiceOption::Gain(GainOption { food: 1, resources: 2 }),
                ChoiceOption::Gain(GainOption { food: 2, resources: 1 }),
                ChoiceOption::Gain(GainOption { food: 3, resources: 0 }),
            ]
        );
    }

    /// Raiders' loss side, reusing `plunder_split_options` against the
    /// player's OWN banks: a player who cannot cover the whole amount from
    /// ONE pool alone (only 1 food, needs to cover up to 2 of the loss) must
    /// not be offered an option that would drive that pool negative --
    /// `(food: 2, resources: 0)` is illegal here and must be absent.
    #[test]
    fn plunder_split_options_omits_a_split_the_players_own_short_pool_cannot_cover() {
        let mut p = blank_player(0);
        p.food = 1;
        p.resources = 5;
        let opts = plunder_split_options(&p, 2);
        assert_eq!(
            opts.as_slice(),
            &[
                ChoiceOption::Gain(GainOption { food: 0, resources: 2 }),
                ChoiceOption::Gain(GainOption { food: 1, resources: 1 }),
            ],
            "food:2 would need a food pool this player does not have -- must not be offered"
        );
    }

    /// `QueueItem::FoodOrResSplit` -> `run_item` opens a REAL
    /// `Pending::Choice`, and `pending_moves` (legal.rs's only caller, the
    /// weighted bot's move source) turns it into ordinary `Move::Choose`
    /// candidates with zero `ChoiceKind`-specific code -- i.e. the bot's
    /// normal board-state evaluation decides this split like any other
    /// move, not a hardcoded heuristic (`legal.rs`'s `pending_moves`).
    #[test]
    fn queued_food_or_res_split_opens_a_pending_choice_pending_moves_can_answer() {
        let mut state = blank_state(2);
        state.players[0].food = 5;
        state.players[0].resources = 5;
        enqueue(&mut state, QueueItem::FoodOrResSplit { player: 0, amount: 2, lose: true });
        run_queue(&mut state);
        let Some(Pending::Choice(c)) = state.pending.top() else { panic!("expected an open FoodOrResSplit choice") };
        assert_eq!(c.player, 0);
        assert_eq!(c.kind, ChoiceKind::FoodOrResSplit { lose: true });
        // Full pools on both sides: r ranges 2..=0, three splits.
        assert_eq!(c.options.len(), 3);
        assert_eq!(
            legal::legal_moves(&state).as_slice(),
            &[Move::Choose { n: 0 }, Move::Choose { n: 1 }, Move::Choose { n: 2 }]
        );
    }

    /// Resolving a LOSS split drains exactly the chosen amounts -- not
    /// `events::food_or_resources`'s old "resources first" formula (which
    /// would have drained all 2 from resources here, leaving food at 5).
    #[test]
    fn resolve_choice_food_or_res_split_loss_drains_the_chosen_split_only() {
        let mut state = blank_state(2);
        state.players[0].food = 5;
        state.players[0].resources = 5;
        let opts = plunder_split_options(&state.players[0], 2);
        let choice = Choice { player: 0, kind: ChoiceKind::FoodOrResSplit { lose: true }, options: opts };
        // Index 1 of `plunder_split_options(printed=2)` on a 5/5 player is
        // the (food: 1, resources: 1) split (r_max=2 at index 0, r=1 next).
        resolve_choice(&mut state, &choice, 1);
        assert_eq!(state.players[0].food, 4, "5 - the chosen 1 food");
        assert_eq!(state.players[0].resources, 4, "5 - the chosen 1 resource");
    }

    /// Resolving a GAIN split is blue-token limited per bank, exactly like
    /// `PlunderSplit`'s own attacker-side arm (`resolve_choice`'s doc on
    /// that variant) -- a chosen split can land short of what was picked.
    #[test]
    fn resolve_choice_food_or_res_split_gain_is_blue_token_limited_like_plunder_split() {
        let mut state = blank_state(2);
        state.players[0].blue_total = 2; // only 2 tokens free in the bank
        let choice = Choice {
            player: 0,
            kind: ChoiceKind::FoodOrResSplit { lose: false },
            options: {
                let mut o = OptionList::new();
                o.push(ChoiceOption::Gain(GainOption { food: 0, resources: 5 }));
                o
            },
        };
        resolve_choice(&mut state, &choice, 0);
        assert_eq!(state.players[0].resources, 2, "capped by the 2 free blue tokens, not the chosen 5");
        assert_eq!(state.players[0].food, 0);
    }

    /// International Agreement (CoL p.12): declining the whole privilege
    /// (never choosing a `Slot`, going straight to `Word(Stop)`) must NOT
    /// cost the player their next Politics Phase -- the forfeit is the
    /// price of USING the privilege, not of merely being offered it. This
    /// is the direct fix for the `IllegalMove: PolPass` bucket's 9-game
    /// "declines international agreement" subgroup (`docs/REPLAY.md`,
    /// 2026-08-14): those humans' own logged, perfectly legal "<Color>
    /// passes Political Phase" the following turn was illegal against this
    /// engine because `events.rs` used to set `skip_next_politics`
    /// unconditionally the moment the event revealed, even for a player who
    /// went on to take zero cards.
    #[test]
    fn resolve_choice_take_row_stop_without_ever_taking_a_card_does_not_forfeit_the_next_politics_phase() {
        let mut state = blank_state(2);
        let mut options = OptionList::new();
        options.push(ChoiceOption::Word(Keyword::Stop));
        let choice = Choice { player: 0, kind: ChoiceKind::TakeRow { budget: 5 }, options };
        resolve_choice(&mut state, &choice, 0);
        assert!(!state.players[0].skip_next_politics, "a full decline must not forfeit the next Politics Phase");
    }

    /// Companion: taking even one card off the row DOES forfeit the
    /// player's next political action -- the card's own text ("...by
    /// giving up its next chance to take a political action"), unaffected
    /// by the fix above.
    #[test]
    fn resolve_choice_take_row_slot_taking_a_card_does_forfeit_the_next_politics_phase() {
        let mut state = blank_state(2);
        state.card_row[2] = card("Iron");
        let mut options = OptionList::new();
        options.push(ChoiceOption::Slot(2));
        options.push(ChoiceOption::Word(Keyword::Stop));
        let choice = Choice { player: 0, kind: ChoiceKind::TakeRow { budget: 5 }, options };
        resolve_choice(&mut state, &choice, 0);
        assert!(state.players[0].skip_next_politics, "taking a card must forfeit the next Politics Phase");
    }
}
