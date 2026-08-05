//! Combat resolution: strength, pact-derived attack legality, war/aggression
//! declaration and (where the type layer and the rest of the engine allow
//! it) resolution. Ports the combat-facing half of `engine/effects.py`
//! (`pacts_for`, `pact_forbids_attack`, `war_forbidden`, `pact_attack_bonus`,
//! `_doomed_pact_strength`, `attack_strength`, `defense_strength`,
//! `cancel_attack_pacts`) plus `engine/events.py`'s `start_aggression`,
//! `finish_aggression` and `resolve_war`.
//!
//! ## What is here, and why it is the single source
//!
//! `legal.rs`'s KNOWN GAPS (as of 2026-08-05, before this module) named
//! `pacts_for`/`pact_forbids_attack`/`attack_strength`/`defense_strength`/
//! `war_forbidden` as missing, which blocked `offer_pact`/`aggression`/`war`
//! move generation entirely. All five are ported below, and `legal.rs`'s own
//! `pacts_for` (a duplicate copy, written when this module did not exist yet)
//! is deleted in favour of [`pacts_for`] here -- DESIGN.md's recurring bug
//! class is exactly two registries agreeing by accident, and a pact-party
//! query is exactly the kind of fact that must live once. Likewise
//! `apply.rs` carried its own `cancel_attack_pacts`, written for `h_war`
//! before this module existed; that copy is deleted too, in favour of
//! [`cancel_attack_pacts`] here.
//!
//! `Stats::war_immune` -- the other piece `legal.rs`'s KNOWN GAPS listed as
//! missing -- turned out to already exist (`effects.rs`'s `apply_pacts`
//! reads a pact's `A`/`B`/`bothPlayers` block for `war_immune` directly, a
//! pact port that landed after that doc comment was written); [`war_forbidden`]
//! just reads `effects::state_stats(..).war_immune`, nothing new needed there.
//!
//! `Card` "prints distinct A/B sides" (`legal.rs`'s fourth blocker on
//! `offer_pact`) is also already recoverable, not still blocked: a pact
//! card's `special` list carries `Special::A(PactBlock)`/`Special::B
//! (PactBlock)` when the printed data has separate `A`/`B` effect blocks
//! (`Card`'s doc comment on the generated `Special` enum), and
//! `data/cards_military_actions.json` confirms the two are exactly
//! equivalent to Python's own `card.get("sides")` flag: every pact printing
//! `"sides": ["A", "B"]` has BOTH an `A` and a `B` effects key, and every
//! pact printing `"sides": null` has neither (it has `bothPlayers` instead)
//! -- verified 2026-08-05 against all ten base-game pact cards. So "has both
//! `Special::A` and `Special::B` in `special`" is the same test as Python's
//! `if sides:`, with no new field needed.
//!
//! ## War spoils: all three kinds ported
//!
//! `rust/tools/gen_cards.py`'s `DEFERRED_DICT_EFFECT_KEYS` dropped
//! `victorTakesYellowTokens` and `victorTakesCulture` to payload-less
//! `Special` variants, reason "war resolution -- combat.rs not ported" --
//! read as "the magnitude lives in the dict, which this module cannot see".
//! That turns out not to be true for either key: `engine/events.py::
//! resolve_war` (lines 666-675) never actually reads either dict at
//! resolution time -- it hardcodes `1 + adv // 5` (territory, capped at the
//! loser's yellow bank) and `5 + adv` (culture, capped at the loser's
//! culture) as Python literals, keyed off the war card's PRINTED name via
//! the `WAR_SPOILS` dict, not off `card.get("effects")` at all. DESIGN.md is
//! explicit that "Python is the spec for the rules, never for the
//! representation" -- these two literals ARE the rule (not a stray
//! optimization of the dict), so [`apply_war_spoils`] hardcodes the same two
//! constants, cited by the same file/line, rather than treating this as
//! blocked on a payload that the actual resolution code does not consult.
//! (The dict values in the data do agree with the hardcoded formula --
//! `{"base": 1, "perStrengthAdvantage": 5}` and `{"base": 5, "plus":
//! "strengthAdvantage"}` -- which is presumably why nobody noticed Python
//! duplicates rather than reads them.)
//!
//! "War over Technology" (`victorTakesScienceUpTo` +
//! `orTakesSpecialTechnologiesOfSameTotalScienceCost`) is a real DECISION:
//! `orTakesSpecialTechnologiesOfSameTotalScienceCost` is printed `true` on
//! the base game's only copy (`data/cards_military_actions.json`), so
//! `events.resolve_war` always routes it through `interact::war_tech_spoils`
//! -- science vs. one or more stealable blue technologies, and FAQ p.8 says
//! mixing is legal. It degrades to the no-decision case (`interact::
//! take_war_science`, a pure `min(budget, loser.science)`) when the loser
//! holds no stealable special technology, and determining THAT needs
//! `interact::war_tech_options`. This was blocked until `interact.rs`
//! existed; it landed 2026-08-05, so [`apply_war_spoils`] now handles all
//! three war kinds and the `unimplemented!` here is gone.
//!
//! ## Aggression: declaration, defense and success effects all ported
//!
//! [`start_aggression`] is the exact portable prefix of `engine/events.py::
//! start_aggression`: pay the (Gandhi-doubled) cost, discard the card,
//! compute the attacker's strength, cancel any pact that ends on mutual
//! attack. `apply::h_aggression` then hands the defense decision over
//! through `interact::start_defense`, and [`finish_aggression`] here is what
//! that decision resolves into once the defender's committed total exists.
//!
//! [`finish_aggression`]'s FAILURE branch was always complete (a log line
//! Python has and this port drops, then `return False`). Its SUCCESS branch
//! (2026-08-05) now is too: it opens with the general `events::apply_gains`
//! (`events.rs` -- the event-gain interpreter -- landed alongside this
//! change) and then walks `takeFromOpponent`/`destroyUrbanBuildings`/
//! `opponentDecreasesPopulation`/`stealColony`/`removeFromGame`, the five
//! per-card payloads `gen_cards.py`'s `DEFERRED_DICT_EFFECT_KEYS`/
//! `DEFERRED_LIST_EFFECT_KEYS` used to collapse to payload-less `Special`s.
//! Two of the five (`opponentDecreasesPopulation`/`stealColony`) already
//! carried a real `(i16)` payload -- nothing but this function was reading
//! them; the other three needed generator work: `takeFromOpponent` got a
//! dedicated `TakeFromOpponentBlock` (three fields, §5.4.6's own closed
//! vocabulary -- see its doc comment in `cards.rs`), `destroyUrbanBuildings`
//! got a real `&'static [Age]` (one entry per raid), and `removeFromGame`
//! stayed a unit variant on purpose: Python only ever tests its PRESENCE
//! (`events.py:679`), never its list contents -- see [`finish_aggression`]'s
//! own doc comment. The QUEUE side of this branch was already ported and
//! waiting: `QueueItem::Raid`/`Annex`/`Infiltrate`/`LosePop` are exactly the
//! four items it enqueues, and `interact::run_item` already resolved all
//! four.

use crate::cards::{CardId, Special};
use crate::economy;
use crate::effects;
use crate::events;
use crate::interact;
use crate::state::{GameState, Pact, PlayerState, QueueItem};

/// Duplicated (not imported) from `costs.rs`'s private `leader_is` -- see
/// that module's "a note on leader identity", and `legal.rs`/`apply.rs`'s own
/// copies for the same reason: it is four lines, and `costs.rs` is not this
/// pass's file to make `pub(crate)`.
#[inline]
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

// ------------------------------------------------------------------- pacts

/// Every pact `idx` is party to, wherever it physically sits (§5.9). Mirrors
/// `engine/effects.py::pacts_for`. THE canonical copy -- `legal.rs` used to
/// carry its own (self-contained, since nothing else needed pact math yet);
/// that copy is deleted in favour of this one now that [`attack_strength`] /
/// [`defense_strength`] / [`pact_forbids_attack`] / [`war_forbidden`] all
/// need it too.
pub fn pacts_for(state: &GameState, idx: u8) -> impl Iterator<Item = &Pact> {
    state.players[..state.num_players as usize]
        .iter()
        .flat_map(|q| q.pacts.as_slice().iter())
        .filter(move |pact| pact.is_party(idx))
}

/// §5.4.2 / §5.6: a pact between `attacker` and `defender` may forbid the
/// attack outright (`noAttacksBetweenParties`). Mirrors `engine/effects.py::
/// pact_forbids_attack`, minus its dead `if war_immune: pass` -- Python's own
/// comment on that line says war-immunity "only blocks wars; checked
/// separately by `war_forbidden()`", i.e. it is deliberately NOT part of
/// this test; [`war_forbidden`] is the one that adds it back in for the war
/// case specifically.
pub fn pact_forbids_attack(state: &GameState, attacker: &PlayerState, defender: &PlayerState) -> bool {
    for pact in pacts_for(state, attacker.idx) {
        if pact.partner_of(attacker.idx) != defender.idx {
            continue;
        }
        if pact.card.get().special.contains(&Special::NoAttacksBetweenParties) {
            return true;
        }
    }
    false
}

/// §5.6: whether `attacker` may declare WAR on `defender` at all -- stricter
/// than [`pact_forbids_attack`] alone, because war-immunity
/// (`cannotBeDeclaredWarOnByAnyone`) blocks a war even with no
/// `noAttacksBetweenParties` pact in play. Mirrors `engine/effects.py::
/// war_forbidden`.
pub fn war_forbidden(state: &GameState, attacker: &PlayerState, defender: &PlayerState) -> bool {
    pact_forbids_attack(state, attacker, defender) || effects::state_stats(state, defender).war_immune
}

/// Strength a pact grants `attacker` ONLY when attacking `defender`
/// specifically (§5.4.2, `onAttackBetweenParties.attackerStrength`). Mirrors
/// `engine/effects.py::pact_attack_bonus`.
fn pact_attack_bonus(state: &GameState, attacker: &PlayerState, defender: &PlayerState) -> i32 {
    let mut total = 0;
    for pact in pacts_for(state, attacker.idx) {
        if pact.partner_of(attacker.idx) != defender.idx {
            continue;
        }
        for &sp in pact.card.get().special {
            if let Special::OnAttackBetweenParties(block) = sp {
                total += block.attacker_strength as i32;
            }
        }
    }
    total
}

/// Strength `idx` (one of `one`/`other`) draws from a pact between them that
/// ends the moment either attacks the other (§5.4.3). FAQ p.11: such a
/// pact's strength "will not affect any War or Aggression ... declared
/// between the two civilizations -- for the Pact is cancelled immediately."
/// That is true of both parties, so the same function serves the attacker
/// and the defender (mirrors `engine/effects.py::_doomed_pact_strength`).
fn doomed_pact_strength(state: &GameState, one: &PlayerState, other: &PlayerState, idx: u8) -> i32 {
    let mut total = 0;
    for pact in pacts_for(state, one.idx) {
        if pact.partner_of(one.idx) != other.idx {
            continue;
        }
        let card = pact.card.get();
        if !card.special.contains(&Special::CancelledIfPartiesAttackEachOther) {
            continue;
        }
        for &sp in card.special {
            match sp {
                Special::BothPlayers(block) => total += block.strength as i32,
                Special::A(block) if idx == pact.a => total += block.strength as i32,
                Special::B(block) if idx == pact.b => total += block.strength as i32,
                _ => {}
            }
        }
    }
    total
}

/// §5.4.2: `attacker`'s strength for an attack on `defender` -- includes a
/// pact's attack-only bonus, excludes strength from a pact that ends the
/// moment they attack each other (so `legal_moves` and resolution agree).
/// Mirrors `engine/effects.py::attack_strength`.
pub fn attack_strength(state: &GameState, attacker: &PlayerState, defender: &PlayerState) -> i32 {
    let mut total = effects::state_stats(state, attacker).strength;
    total += pact_attack_bonus(state, attacker, defender);
    total -= doomed_pact_strength(state, attacker, defender, attacker.idx);
    total.max(0)
}

/// §5.4.2: `defender`'s strength for the legality comparison -- the mirror
/// image of [`attack_strength`]. A doomed pact is excluded here too, for the
/// same reason: `cancel_attack_pacts` removes it before resolution, so the
/// "may I attack at all?" test must already assume it gone, or a legal
/// attack could read as illegal by up to the pact's strength. Mirrors
/// `engine/effects.py::defense_strength`.
pub fn defense_strength(state: &GameState, attacker: &PlayerState, defender: &PlayerState) -> i32 {
    let mut total = effects::state_stats(state, defender).strength;
    total -= doomed_pact_strength(state, attacker, defender, defender.idx);
    total.max(0)
}

/// §5.4.3 / CoL p.4: a pact that ends the moment its two parties attack each
/// other is removed before the attack resolves. Mirrors `engine/effects.py::
/// cancel_attack_pacts`. THE canonical copy -- `apply.rs::h_war` used to
/// carry its own (written before this module existed); deleted in favour of
/// this one.
pub fn cancel_attack_pacts(state: &mut GameState, attacker_idx: u8, defender_idx: u8) {
    for q in state.players.iter_mut() {
        q.pacts.retain(|pact| {
            let both_parties = pact.is_party(attacker_idx) && pact.is_party(defender_idx);
            !(both_parties && pact.card.get().special.contains(&Special::CancelledIfPartiesAttackEachOther))
        });
    }
}

// -------------------------------------------------------------- aggression

/// §5.4.1-5.4.3: the portable prefix of `engine/events.py::start_aggression`
/// -- pay the (Gandhi-doubled) military-action cost, discard the card from
/// the attacker's hand, compute the attacker's strength, and cancel any pact
/// that ends on mutual attack. Returns the attacker's strength for the
/// defender to be compared against once a defense total exists.
///
/// Stops exactly where Python calls `interact.start_defense` -- see this
/// module's top doc comment "Aggression: declaration ported, resolution
/// blocked on `interact.rs`" for why nothing further belongs here.
pub fn start_aggression(state: &mut GameState, attacker_idx: u8, card: CardId, defender_idx: u8) -> i32 {
    let mut cost = card.get().military_action_cost as i32;
    if leader_is(&state.players[defender_idx as usize], "Mahatma Gandhi") {
        cost *= 2;
    }
    state.players[attacker_idx as usize].military_actions -= cost as i8;
    state.players[attacker_idx as usize].hand_military.remove_first(card);
    economy::discard_military(state, card);
    let atk = {
        let attacker = &state.players[attacker_idx as usize];
        let defender = &state.players[defender_idx as usize];
        attack_strength(state, attacker, defender)
    };
    cancel_attack_pacts(state, attacker_idx, defender_idx);
    atk
}

// -------------------------------------------------------------------- wars

/// The victor, loser and strength advantage of a resolved war -- everything
/// [`resolve_war_outcome`] can determine without a war-spoils payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarOutcome {
    pub victor: u8,
    pub loser: u8,
    /// `victor`'s strength minus `loser`'s, at resolution (always > 0).
    pub advantage: i32,
    pub card: CardId,
}

/// §5.7: resolve the war `attacker_idx` declared last turn, up to (not
/// including) applying its spoils -- see [`apply_war_spoils`] for that half,
/// and this module's top doc comment for exactly which war kinds it can
/// handle. Mirrors `engine/events.py::resolve_war` through the tie/victor/
/// loser/advantage computation, the war card's discard, and clearing both
/// players' war-tracking fields.
///
/// `None` if `attacker_idx` has no war declared (Python's `if not war:
/// return`) or the war is a dead tie (`a == d`; Python's plain `return`, no
/// spoils either way, but the card is still discarded and the fields still
/// cleared -- both happen before the tie check in Python and here).
///
/// Deliberately NOT `attack_strength`/`defense_strength`: those include a
/// doomed-pact-strength subtraction that no longer applies here -- the pact
/// was already cancelled back when the war was DECLARED (`h_war` calls
/// [`cancel_attack_pacts`] immediately), so by resolution time there is
/// nothing left to subtract, and Python's `resolve_war` (events.py:655-657)
/// reads `state_stats(...).strength` directly rather than routing through
/// either function, for exactly that reason.
pub fn resolve_war_outcome(state: &mut GameState, attacker_idx: u8) -> Option<WarOutcome> {
    let card = state.players[attacker_idx as usize].war_declared_by_me;
    if card.is_none() {
        return None;
    }
    let target = state.players[attacker_idx as usize].war_target;
    state.players[attacker_idx as usize].war_declared_by_me = CardId::NONE;
    // `war_target` is documented "meaningless while `war_declared_by_me` is
    // NONE", but it is still a FIELD, and the differential replay compares
    // fields: a stale target survives into the next snapshot and reads as a
    // divergence (`2p_seed101.jsonl` ply 133, found the moment `interact.rs`
    // made mid-war-spoils states loadable). Python has no such field at all
    // -- its `war_declared_by_me` is one tuple that becomes `None` -- so
    // clearing both together is what makes the two representations agree.
    state.players[attacker_idx as usize].war_target = 0;
    state.players[target as usize].wars_declared_on_me[attacker_idx as usize] = CardId::NONE;

    let a = {
        let attacker = &state.players[attacker_idx as usize];
        let defender = &state.players[target as usize];
        effects::state_stats(state, attacker).strength + pact_attack_bonus(state, attacker, defender)
    };
    let d = effects::state_stats(state, &state.players[target as usize]).strength;
    economy::discard_military(state, card);
    if a == d {
        return None;
    }
    let (victor, loser, advantage) = if a > d { (attacker_idx, target, a - d) } else { (target, attacker_idx, d - a) };
    Some(WarOutcome { victor, loser, advantage, card })
}

/// §5.7 war spoils. See this module's top doc comment "War spoils: two of
/// three kinds ported, one still blocked (with why)" for exactly what each
/// branch does and does not need. `outcome.card`'s PRINTED name selects the
/// kind (`Card::base_name`'s own doc comment: "rules that key on the printed
/// name ... read THIS, not `name`"), matching Python's `WAR_SPOILS` dict.
pub fn apply_war_spoils(state: &mut GameState, outcome: &WarOutcome) {
    match outcome.card.get().base_name {
        // events.py:666-669.
        "War over Territory" => {
            let loser_bank = state.players[outcome.loser as usize].yellow_bank as i32;
            let take = (1 + outcome.advantage / 5).min(loser_bank);
            state.players[outcome.loser as usize].yellow_bank -= take as u8;
            crate::apply::grant_yellow(&mut state.players[outcome.victor as usize], take);
        }
        // events.py:672-675.
        "War over Culture" => {
            let loser_culture = state.players[outcome.loser as usize].culture as i32;
            let take = (5 + outcome.advantage).min(loser_culture);
            state.players[outcome.loser as usize].culture -= take as u16;
            state.players[outcome.victor as usize].culture += take as u16;
        }
        // events.py:690-697. Gated on the CARD's own effect key rather than
        // on the spoils kind, exactly as Python is, so the alternative spoil
        // stays a property of the data: a war card that pays science with no
        // such clause takes the science with no decision. The base game's
        // only War over Technology prints the clause, so the first arm is
        // the live one and the second is parity, not dead weight.
        "War over Technology" => {
            if outcome
                .card
                .get()
                .special
                .contains(&Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost)
            {
                interact::war_tech_spoils(state, outcome.victor, outcome.loser, outcome.advantage);
            } else {
                interact::take_war_science(
                    state,
                    outcome.victor,
                    outcome.loser,
                    outcome.advantage,
                );
            }
        }
        other => unimplemented!(
            "war spoils for {other}: no rule known for this printed name -- \
             `engine/events.py::WAR_SPOILS` maps exactly three, and this is \
             not one of them",
        ),
    }
}

// -------------------------------------------------- aggression resolution

/// §5.4.5-5.4.6: compare the two totals and resolve the card. Mirrors
/// `engine/events.py::finish_aggression`, and is called by
/// `interact::start_defense` / `interact::defense_move` once the defender's
/// committed total exists (which is the thing that did not exist before
/// `state.pending` did -- see this module's top doc comment).
///
/// Returns whether the aggression SUCCEEDED. The failure branch is a log
/// line and `return False` in Python, nothing more -- there is no logging
/// port (see `economy.rs`'s equivalent note), so it is just the early
/// return here.
///
/// The success branch opens with the general `events::apply_gains` (the
/// attacker's own gains, e.g. Enslave's `gainFood`/`gainResources`), then
/// walks the five per-card payloads `rust/tools/gen_cards.py` used to
/// collapse to payload-less `Special`s before this pass:
/// `takeFromOpponent`/`destroyUrbanBuildings` (now real payloads --
/// `TakeFromOpponentBlock`/`&'static [Age]`), `opponentDecreasesPopulation`/
/// `stealColony` (already int-shape `Special`s -- nothing but this function
/// was reading them), and `removeFromGame` (still a unit `Special`, read for
/// PRESENCE only, matching `events.py:679`'s own `if eff.get(...)` -- see
/// `gen_cards.py`'s `LIST_PRESENCE_EFFECT_KEYS`).
///
/// The queue side was already ported and waiting: `QueueItem::Raid` /
/// `Annex` / `Infiltrate` / `LosePop` are exactly the four items
/// `finish_aggression` enqueues, and `interact::run_item` resolves all four.
pub fn finish_aggression(state: &mut GameState, ctx: &crate::state::Defense) -> bool {
    if ctx.dfn >= ctx.atk {
        return false;
    }
    let attacker = ctx.attacker;
    let defender = ctx.player;
    let card = ctx.card;

    // events.py:650: the attacker's own gains off the card's top-level
    // effects (e.g. Enslave's gainFood/gainResources).
    events::apply_gains(state, attacker, card, 1);

    // events.py:651-667: steal from the defender. Only three sub-keys are
    // ever read (see `TakeFromOpponentBlock`'s doc comment); a zero field
    // means that sub-key was not printed on this card.
    if let Some(&Special::TakeFromOpponent(block)) =
        card.get().special.iter().find(|s| matches!(s, Special::TakeFromOpponent(_)))
    {
        if block.food_and_or_resources != 0 {
            let before_f = state.players[defender as usize].food;
            let before_r = state.players[defender as usize].resources;
            events::food_or_resources(
                &mut state.players[defender as usize],
                block.food_and_or_resources as i32,
                -1,
            );
            let moved = (before_f - state.players[defender as usize].food) as i32
                + (before_r - state.players[defender as usize].resources) as i32;
            events::food_or_resources(&mut state.players[attacker as usize], moved, 1);
        }
        if block.science != 0 {
            let moved = (block.science as u16).min(state.players[defender as usize].science);
            state.players[defender as usize].science -= moved;
            state.players[attacker as usize].science += moved;
        }
        if block.culture != 0 {
            let moved = (block.culture as u16).min(state.players[defender as usize].culture);
            state.players[defender as usize].culture -= moved;
            state.players[attacker as usize].culture += moved;
        }
    }

    // events.py:668-671.
    if let Some(&Special::OpponentDecreasesPopulation(n)) = card
        .get()
        .special
        .iter()
        .find(|s| matches!(s, Special::OpponentDecreasesPopulation(_)))
    {
        if n != 0 {
            interact::enqueue(state, QueueItem::LosePop { player: defender, n: n as u8 });
        }
    }

    // events.py:672-675: one raid per printed spec, in order, WITH loot
    // (Python passes no `no_loot` key, and `interact.py::_q_raid` defaults
    // `loot = not item.get("no_loot")` to `True` when the key is absent --
    // this IS the Raid aggression card's whole point, matching
    // `QueueItem::Raid`'s `no_loot: false`).
    if let Some(&Special::DestroyUrbanBuildings(ages)) =
        card.get().special.iter().find(|s| matches!(s, Special::DestroyUrbanBuildings(_)))
    {
        for &age in ages {
            interact::enqueue(
                state,
                QueueItem::Raid { player: attacker, victim: defender, max_age: age, no_loot: false },
            );
        }
    }

    // events.py:676-678.
    if let Some(&Special::StealColony(n)) =
        card.get().special.iter().find(|s| matches!(s, Special::StealColony(_)))
    {
        if n != 0 {
            interact::enqueue(state, QueueItem::Annex { player: attacker, victim: defender });
        }
    }

    // events.py:679-683: `removeFromGame`'s list contents are never read (see
    // this function's doc comment) -- only its presence, and the per-level
    // culture rate, defaulting to 3 exactly as Python's `_num(...) or 3`
    // does (a PRINTED `gainCulturePerLevelOfRemovedCard: 0` would also fall
    // back to 3, since Python's `or` treats 0 as falsy too).
    if card.get().special.contains(&Special::RemoveFromGame) {
        let per = card
            .get()
            .special
            .iter()
            .find_map(|s| match s {
                Special::GainCulturePerLevelOfRemovedCard(n) if *n != 0 => Some(*n as u8),
                _ => None,
            })
            .unwrap_or(3);
        interact::enqueue(state, QueueItem::Infiltrate { player: attacker, victim: defender, per });
    }

    true
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{CardId, CardType};
    use crate::state::{
        CardList, GameState, Pact, PactList, Phase, PlayerState, Tableau, MAX_PLAYERS, ROW_SIZE,
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
            caesar_second_politics: false,
            peeked_event: CardId::NONE,
            ca_penalty_next_turn: 0,
            mil_discount: 0,
            mil_sci_discount: 0,
            one_time_discount: crate::state::OneTimeDiscount::default(),
            resigned: false,
        }
    }

    fn blank_state(num_players: u8, players: [PlayerState; MAX_PLAYERS]) -> GameState {
        GameState {
            num_players,
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
            seeded_by: [crate::state::NOT_SEEDED; crate::cards::NUM_CARDS],
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

    fn two_player_state(p0: PlayerState, p1: PlayerState) -> GameState {
        let filler = || blank_player(2, card("Despotism"));
        let mut players = [filler(), filler(), filler(), filler()];
        players[0] = p0;
        players[1] = p1;
        blank_state(4, players)
    }

    fn pact_card_with_no_attacks() -> CardId {
        // "Acceptance of Supremacy" / "Loss of Sovereignty" both print
        // `noAttacksBetweenParties` alongside `A`/`B`; any works.
        card("Peace Treaty")
    }

    // ------------------------------------------------------------ pacts_for

    #[test]
    fn pacts_for_finds_a_pact_regardless_of_which_player_physically_holds_it() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.pacts.push(Pact { card: card("Peace Treaty"), owner: 1, partner: 0, a: 0, b: 1 });
        let state = two_player_state(p0, p1);
        let found: Vec<_> = pacts_for(&state, 0).collect();
        assert_eq!(found.len(), 1, "player 0 is a party even though player 1 holds the card");
    }

    // ----------------------------------------------------- pact_forbids_attack

    #[test]
    fn pact_forbids_attack_true_for_a_no_attacks_pact_between_the_two() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.pacts.push(Pact { card: pact_card_with_no_attacks(), owner: 0, partner: 1, a: 0, b: 1 });
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);
        assert!(pact_forbids_attack(&state, &state.players[0], &state.players[1]));
    }

    #[test]
    fn pact_forbids_attack_false_with_no_pact_between_the_two() {
        let p0 = blank_player(0, card("Despotism"));
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);
        assert!(!pact_forbids_attack(&state, &state.players[0], &state.players[1]));
    }

    #[test]
    fn pact_forbids_attack_ignores_a_pact_with_a_different_partner() {
        let mut p0 = blank_player(0, card("Despotism"));
        // Owner/partner are 0/2, not 0/1 -- irrelevant to an attack on player 1.
        p0.pacts.push(Pact { card: pact_card_with_no_attacks(), owner: 0, partner: 2, a: 0, b: 2 });
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);
        assert!(!pact_forbids_attack(&state, &state.players[0], &state.players[1]));
    }

    // ----------------------------------------------------------- war_forbidden

    #[test]
    fn war_forbidden_true_when_defender_is_war_immune() {
        // "Loss of Sovereignty" prints `cannotBeDeclaredWarOnByAnyone` on its
        // B side only (data/cards_military_actions.json) -- so a pact with
        // `b == defender_idx` makes the defender war-immune, with no
        // `noAttacksBetweenParties`-style attack restriction involved at all
        // (this exercises `war_forbidden`'s OWN `war_immune` check, not
        // `pact_forbids_attack`, which `pact_forbids_attack_true_for_a_no_attacks_pact_between_the_two`
        // above already covers).
        let card_id = card("Loss of Sovereignty");
        let b_has_war_immune = card_id.get().special.iter().any(|s| matches!(s, Special::B(b) if b.war_immune));
        assert!(b_has_war_immune, "test fixture assumption: Loss of Sovereignty's B side grants war immunity");

        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.pacts.push(Pact { card: card_id, owner: 1, partner: 0, a: 0, b: 1 }); // b == defender's idx (1)
        let state = two_player_state(p0, p1);
        assert!(war_forbidden(&state, &state.players[0], &state.players[1]));
    }

    #[test]
    fn war_forbidden_false_with_no_pact_and_no_immunity() {
        let p0 = blank_player(0, card("Despotism"));
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);
        assert!(!war_forbidden(&state, &state.players[0], &state.players[1]));
    }

    // ------------------------------------------------- attack/defense strength

    #[test]
    fn attack_strength_is_plain_stats_strength_with_no_pacts() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.techs.insert(card("Warriors"), crate::state::TechSlot { workers: 3, stored: 0 });
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);
        let s = effects::state_stats(&state, &state.players[0]).strength;
        assert_eq!(attack_strength(&state, &state.players[0], &state.players[1]), s);
    }

    #[test]
    fn doomed_pact_strength_is_excluded_from_both_attack_and_defense() {
        // A pact whose bothPlayers block grants strength AND ends on mutual
        // attack -- "Military Alliance" (bothPlayers + cancelledIfPartiesAttackEachOther).
        let mac = card("Military Alliance");
        let block_strength = mac
            .get()
            .special
            .iter()
            .find_map(|s| if let Special::BothPlayers(b) = s { Some(b.strength as i32) } else { None })
            .expect("Military Alliance grants strength via bothPlayers");
        assert!(block_strength > 0, "test needs a nonzero strength grant to be meaningful");

        let mut p0 = blank_player(0, card("Despotism"));
        p0.pacts.push(Pact { card: mac, owner: 0, partner: 1, a: 0, b: 1 });
        let p1 = blank_player(1, card("Despotism"));
        let state = two_player_state(p0, p1);

        let base = effects::state_stats(&state, &state.players[0]).strength;
        // compute() DOES count this pact (it is not attack-cancelled from the
        // Stats point of view), so plain `state_stats` includes it...
        assert_eq!(base, block_strength, "sanity: the pact IS the player's whole strength here");
        // ...but attack_strength against the SAME partner must exclude it,
        // since it would be cancelled the instant this attack happens.
        assert_eq!(attack_strength(&state, &state.players[0], &state.players[1]), 0);
        assert_eq!(defense_strength(&state, &state.players[0], &state.players[1]), 0);
    }

    // -------------------------------------------------------- cancel_attack_pacts

    #[test]
    fn cancel_attack_pacts_removes_only_the_matching_pact() {
        let mut p0 = blank_player(0, card("Despotism"));
        let doomed = card("Military Alliance");
        let survives = card("Scientific Cooperation"); // bothPlayers, no cancellation clause
        p0.pacts.push(Pact { card: doomed, owner: 0, partner: 1, a: 0, b: 1 });
        p0.pacts.push(Pact { card: survives, owner: 0, partner: 1, a: 0, b: 1 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = two_player_state(p0, p1);
        cancel_attack_pacts(&mut state, 0, 1);
        let remaining: Vec<_> = state.players[0].pacts.as_slice().iter().map(|p| p.card).collect();
        assert_eq!(remaining, vec![survives]);
    }

    // ------------------------------------------------------------ start_aggression

    #[test]
    fn start_aggression_pays_cost_discards_and_returns_attacker_strength() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.military_actions = 4;
        let raid = crate::cards::CARDS
            .iter()
            .position(|c| c.kind == CardType::Aggression)
            .map(|i| CardId(i as u16))
            .expect("at least one aggression card exists");
        p0.hand_military.push(raid);
        p0.techs.insert(card("Warriors"), crate::state::TechSlot { workers: 2, stored: 0 });
        let p1 = blank_player(1, card("Despotism"));
        let mut state = two_player_state(p0, p1);
        let cost = raid.get().military_action_cost as i8;

        let atk = start_aggression(&mut state, 0, raid, 1);
        assert_eq!(atk, effects::state_stats(&state, &state.players[0]).strength);
        assert_eq!(state.players[0].military_actions, 4 - cost);
        assert!(!state.players[0].hand_military.contains(raid), "card leaves the hand");
        assert!(
            state.discarded_military[raid.get().age as usize].contains(raid),
            "card is discarded immediately, not held pending resolution"
        );
    }

    #[test]
    fn start_aggression_doubles_cost_against_mahatma_gandhi() {
        let mut p0 = blank_player(0, card("Despotism"));
        p0.military_actions = 10;
        let raid = crate::cards::CARDS
            .iter()
            .position(|c| c.kind == CardType::Aggression)
            .map(|i| CardId(i as u16))
            .expect("at least one aggression card exists");
        p0.hand_military.push(raid);
        let mut p1 = blank_player(1, card("Despotism"));
        p1.leader = card("Mahatma Gandhi");
        let mut state = two_player_state(p0, p1);
        let cost = raid.get().military_action_cost as i8;

        start_aggression(&mut state, 0, raid, 1);
        assert_eq!(state.players[0].military_actions, 10 - 2 * cost);
    }

    // ----------------------------------------------------------- resolve_war

    #[test]
    fn resolve_war_outcome_none_when_no_war_declared() {
        let p0 = blank_player(0, card("Despotism"));
        let p1 = blank_player(1, card("Despotism"));
        let mut state = two_player_state(p0, p1);
        assert!(resolve_war_outcome(&mut state, 0).is_none());
    }

    #[test]
    fn resolve_war_outcome_none_on_a_tie_but_still_clears_and_discards() {
        let mut p0 = blank_player(0, card("Despotism"));
        let war = card("War over Culture");
        p0.war_declared_by_me = war;
        p0.war_target = 1;
        let mut p1 = blank_player(1, card("Despotism"));
        p1.wars_declared_on_me[0] = war;
        let mut state = two_player_state(p0, p1);
        assert!(resolve_war_outcome(&mut state, 0).is_none(), "equal strength (both 0) is a tie");
        assert!(state.players[0].war_declared_by_me.is_none());
        assert!(state.players[1].wars_declared_on_me[0].is_none());
        assert!(state.discarded_military[war.get().age as usize].contains(war));
    }

    #[test]
    fn resolve_war_outcome_picks_the_stronger_side_and_reports_the_advantage() {
        let mut p0 = blank_player(0, card("Despotism"));
        let war = card("War over Culture");
        p0.war_declared_by_me = war;
        p0.war_target = 1;
        p0.techs.insert(card("Warriors"), crate::state::TechSlot { workers: 5, stored: 0 });
        let mut p1 = blank_player(1, card("Despotism"));
        p1.wars_declared_on_me[0] = war;
        let mut state = two_player_state(p0, p1);
        let outcome = resolve_war_outcome(&mut state, 0).expect("attacker is strictly stronger");
        assert_eq!(outcome.victor, 0);
        assert_eq!(outcome.loser, 1);
        assert_eq!(outcome.advantage, 5);
        assert_eq!(outcome.card, war);
    }

    // ------------------------------------------------------- apply_war_spoils

    #[test]
    fn apply_war_spoils_territory_formula_matches_events_py() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.yellow_bank = 10;
        let mut state = two_player_state(p0, p1);
        let outcome = WarOutcome { victor: 0, loser: 1, advantage: 12, card: card("War over Territory") };
        apply_war_spoils(&mut state, &outcome);
        // 1 + 12 // 5 = 3.
        assert_eq!(state.players[1].yellow_bank, 7);
        assert_eq!(state.players[0].yellow_bank, 3);
    }

    #[test]
    fn apply_war_spoils_territory_is_capped_at_the_losers_bank() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.yellow_bank = 2;
        let mut state = two_player_state(p0, p1);
        let outcome = WarOutcome { victor: 0, loser: 1, advantage: 100, card: card("War over Territory") };
        apply_war_spoils(&mut state, &outcome);
        assert_eq!(state.players[1].yellow_bank, 0);
        assert_eq!(state.players[0].yellow_bank, 2);
    }

    #[test]
    fn apply_war_spoils_culture_formula_matches_events_py() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.culture = 50;
        let mut state = two_player_state(p0, p1);
        let outcome = WarOutcome { victor: 0, loser: 1, advantage: 7, card: card("War over Culture") };
        apply_war_spoils(&mut state, &outcome);
        // 5 + 7 = 12.
        assert_eq!(state.players[1].culture, 38);
        assert_eq!(state.players[0].culture, 12);
    }

    /// War over Technology used to panic here (`interact.rs` did not exist).
    /// With no stealable blue technology in the loser's play area there is no
    /// decision to make, so the whole advantage is taken as science -- FAQ
    /// p.8's cap included.
    #[test]
    fn apply_war_spoils_technology_with_nothing_stealable_takes_science() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.science = 10;
        let mut state = two_player_state(p0, p1);
        let outcome = WarOutcome { victor: 0, loser: 1, advantage: 3, card: card("War over Technology") };
        apply_war_spoils(&mut state, &outcome);
        assert!(state.pending.is_empty(), "no decision when nothing is stealable");
        assert_eq!(state.players[0].science, 3);
        assert_eq!(state.players[1].science, 7);
    }

    /// ...and with one, the victor gets a real choice: science, or the card.
    /// `interact::WAR_TECH_SCIENCE_IDX` pins science at index 0.
    #[test]
    fn apply_war_spoils_technology_offers_a_choice_when_something_is_stealable() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.science = 10;
        p1.techs.insert(card("Cartography"), crate::state::TechSlot::default()); // techCost 4
        let mut state = two_player_state(p0, p1);
        let outcome = WarOutcome { victor: 0, loser: 1, advantage: 6, card: card("War over Technology") };
        apply_war_spoils(&mut state, &outcome);
        assert_eq!(state.decider(), 0, "the VICTOR answers, whoever is to move");
        assert_eq!(crate::legal::legal_moves(&state).len(), 2, "science, or Cartography");
    }

    /// Cross-checked directly against `tests/test_combat.py::
    /// TestWar::test_the_victor_may_mix_cards_and_science` (confirmed passing
    /// 2026-08-05, `python3.13 -m pytest tests/test_combat.py -k
    /// victor_may_mix`), which is itself the FAQ p.8 example verbatim: *"As
    /// long as you win enough Science points you can always choose to take
    /// some or all of them in blue Special Technologies"* -- the digital
    /// edition's own log for a 26-vs-14 win takes Code of Laws (cost 6) +
    /// Cartography (cost 4) + 2 science out of a 12-point advantage. Same
    /// setup here (advantage 12, loser holds both cards, loser starts with 30
    /// science), same two steals in the same order, same result: (p0, p1)
    /// science = (2, 28) and both cards end up in the victor's play area.
    #[test]
    fn apply_war_spoils_technology_the_victor_may_mix_cards_and_science() {
        let p0 = blank_player(0, card("Despotism"));
        let mut p1 = blank_player(1, card("Despotism"));
        p1.science = 30;
        p1.techs.insert(card("Code of Laws"), crate::state::TechSlot::default()); // techCost 6
        p1.techs.insert(card("Cartography"), crate::state::TechSlot::default()); // techCost 4
        let mut state = two_player_state(p0, p1);
        let outcome = WarOutcome { victor: 0, loser: 1, advantage: 12, card: card("War over Technology") };
        apply_war_spoils(&mut state, &outcome);

        // Option 0 is science; the two cards are offered most-expensive
        // first, so Code of Laws (6) is index 1 and Cartography (4) is index 2.
        interact::apply_pending(&mut state, crate::moves::Move::Choose { n: 1 }); // Code of Laws: 6 of 12
        assert!(state.players[0].techs.has(card("Code of Laws")));
        assert!(!state.players[1].techs.has(card("Code of Laws")));

        interact::apply_pending(&mut state, crate::moves::Move::Choose { n: 1 }); // Cartography: 4 of the last 6
        assert!(state.players[0].techs.has(card("Cartography")));
        assert!(!state.players[1].techs.has(card("Cartography")));

        assert!(state.pending.is_empty(), "nothing left to steal");
        assert_eq!((state.players[0].science, state.players[1].science), (2, 28));
    }
}
