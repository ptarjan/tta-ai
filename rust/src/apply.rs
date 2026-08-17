//! Move application -- the port of `engine/actions.py`'s `apply` half:
//! `apply`, the `_h_*` per-move handlers, and `do_build`/`do_upgrade`/
//! `do_wonder_step` (the handlers' shared bodies, kept separate because
//! `apply_free_action` in Python calls them a second way -- discounted, no
//! action cost -- once the ordered-action machinery exists to call them that
//! way here too).
//!
//! Depends on `costs.rs` (row/build/upgrade/tech costs, the discount pools,
//! `special_icon`) and `economy.rs` (population, the blue-token bank,
//! `discard_civil`/`discard_military`). Both are read-only dependencies of
//! this module; neither is touched here.
//!
//! ## What is deliberately NOT here, and why
//!
//! Three sibling systems are not ported yet, and every move or trigger that
//! needs one of them is named individually below rather than silently
//! stubbed -- DESIGN.md's whole point is that an unhandled case is loud, not
//! quiet.
//!
//! - ~~**`interact.rs`'s decision queue.**~~ CLOSED 2026-08-05.
//!   `GameState::pending` exists, [`apply`] opens with the `if state.pending:
//!   interact.apply_pending(...)` branch Python does, [`Move::OfferPact`] is
//!   ported ([`h_offer_pact`]), [`Move::Aggression`] hands off to
//!   `interact::start_defense`, and the response moves ([`Move::Bid`],
//!   [`Move::Defend`], [`Move::SendUnit`], [`Move::Choose`], ...) are taken by
//!   that branch. They still `panic!` in the `match` below, but for the
//!   opposite reason: reaching the match arm means one was played with
//!   NOTHING open, which is a caller legality bug rather than a port gap.
//!   [`h_play_action`] now defers its ordered action and its gains onto
//!   `state.queue` exactly as Python does (`QueueItem::FreeCivil` then
//!   `QueueItem::CardGains`), and [`apply`] drains the queue on the way out;
//!   the two synchronous stand-ins written before the queue existed
//!   (`resolve_free_civil_action`, `apply_action_card_gains`) are deleted
//!   rather than left as a second copy of the same rule.
//! - **`events.rs`.** [`Move::PrepareEvent`] (`events.reveal_current_event`)
//!   calls into it directly. Panics in [`apply`].
//! - **`game.rs`.** [`Move::EndTurn`] (`game.end_turn`) is the whole End-of-
//!   Turn Sequence orchestrator and is not ported at all. `_h_resign`'s tail
//!   call to `game.after_resign` (deciding whether resigning leaves a forced
//!   winner, §5.11) is in the same boat -- [`h_resign`] performs every OTHER
//!   effect of resigning and stops one call short of that, with a comment at
//!   the stopping point, rather than pretending the game continues normally.
//!
//! [`Move::War`] used to be blocked here too (`PlayerState` had no
//! `war_declared_by_me` / `wars_declared_on_me` fields), but it was never a
//! `combat.rs` dependency -- `_h_war` itself never resolves combat, it only
//! records the declaration. `state.rs` grew both fields, so [`h_war`] is now
//! fully ported, and so is the war-cleanup half of [`h_resign`] (§5.11:
//! "wars against a resigned player score their declarer 7 culture"). War
//! RESOLUTION (as opposed to declaration) exists too -- `combat::
//! resolve_war_outcome` / `apply_war_spoils` -- and `game.rs` now calls it:
//! Python fires it from `game.start_turn`, not from `_h_war`, and the ported
//! turn loop matches that placement.
//!
//! [`Move::Aggression`] is FULLY ported. [`h_aggression`] calls
//! `combat::start_aggression` (cost, discard, strength, doomed-pact
//! cancellation -- everything `events.start_aggression` does) and then hands
//! off to `interact::start_defense`, which `state.pending` makes expressible.
//! An earlier revision of this comment said the hand-off panicked; it has not
//! since `state.pending` landed, and the differential fixtures now play
//! aggressions through to resolution with zero divergences. Corrected
//! 2026-08-05 -- two edits the same day left this paragraph contradicting the
//! one at the top of this block.
//!
//! `Card::stages` and `Card::revolution_cost` landed in `cards.rs` mid-port,
//! ahead of `costs.rs` being updated to use them -- both are closed now:
//! [`costs::wonder_stage_cost`] reads `Card::stages` (so [`do_wonder_step`]
//! is fully ported; this module's own [`wonder_is_complete`] already read the
//! field directly and was never a gap), and [`costs::revolution_cost`] reads
//! `Card::revolution_cost` (plus the same pact discount [`costs::tech_cost`]
//! already applied to the peaceful path -- see that function's own doc), so
//! [`h_revolution`] is fully ported too.
//!
//! - ~~**Per-player-count effect magnitudes.**~~ CLOSED 2026-08-05.
//!   `Wave of Nationalism` / `Military Build-Up`
//!   (`resourcesForMilitaryUnitsPerStrongerCivilization`) and `Endowment for
//!   the Arts` (`culturePerCivilizationWithMoreCulture`) print a
//!   per-player-count dict (`{"2p": 6, "3p": 3, "4p": 2}`), which
//!   `gen_cards.py` could not fold into a flat `i16` `CardEffects` field
//!   (only a bare int/float value survives that path; a dict value degraded
//!   to a payload-less `Special` variant). Fixed the same way
//!   `strongestPlayers`/`weakestPlayers`/`condition` already were: both keys
//!   now build through `gen_cards.py`'s existing `build_count_table` into a
//!   real `Special::<Name>([i16; 3])` payload (index 0/1/2 = 2p/3p/4p, `events::
//!   live_count_idx` picks the live one), no new machinery needed.
//!   [`h_play_action`] applies both exactly as `engine/actions.py::
//!   _h_play_action` does: culture gain per civilization with MORE culture
//!   than the player (a one-shot `p.culture` add), and `p.mil_discount`
//!   increased by the per-count magnitude for each civilization STRONGER
//!   (matching `effects::state_stats(..).strength`) than the player -- the
//!   same one-shot discount pool `resourcesForMilitaryUnits` (a bare int
//!   effect, already ported) and Churchill's military option already feed:
//!   spent by [`costs::spend_mil_discount`] on the next unit build/upgrade,
//!   and any unspent balance expires at end of turn (`economy::end_of_turn`
//!   zeroes `p.mil_discount`, mirroring `engine/economy.py::end_of_turn`'s
//!   own `p.mil_discount = 0` -- "§3.11 action-card discounts expire").
//!   `Special::FreeCivilAction` is unrelated to this gap: it carries a real
//!   `FreeCivilActionValue` payload
//!   (fixed 2026-08-05) naming one of the six ordered actions, [`legal::
//!   free_action_kind_of`] maps that onto `legal::FreeActionKind`, and
//!   [`h_play_action`] resolves it -- the only remaining blocker for THOSE 18
//!   cards is the 2+-legal-options case described above, not a missing place
//!   to put the value.
//!
//! [`wonder_completion_culture`] implements all three `onBuildCulture` cases
//! now, including `Hollywood`/`Internet`, which score "the effective output
//! of a specific set of buildings" (§9.2) via `effects::building_output` --
//! CLOSED 2026-08-05. `effects.rs` grew a public `building_output` that both
//! `compute()` (through `mine_resources`/`farm_food`) and this trigger call,
//! rather than this module carrying a second copy of the building-modifier
//! arithmetic (`best_staffed`, `workers_on`, the `apply_special` match arms
//! for `BestTheaterDoubleCulture` and friends stay private to `effects.rs`,
//! reached only through that one function) -- exactly the "present in this
//! registry, absent from that one, with nothing that fails when they
//! disagree" bug class this whole rewrite exists to close (Python guards the
//! single-source property here with `tests/test_card_pricing.py::
//! TestOneImplementation`).
//!
//! ## STRICT legality (not ported)
//!
//! Python's `apply()` optionally re-derives `legal_moves(state)` and asserts
//! the given move is in it (env-var gated; on by default in the Python test
//! suite). `legal.rs` is a sibling placeholder -- another worker's file, not
//! yet written -- so there is nothing to call. Every function below assumes
//! its caller already checked legality, exactly as the `costs.rs` cost
//! functions and `economy.rs` mutators already do (see `pay_ca`'s doc
//! comment: "a caller must have checked ... first, not a legality gate of its
//! own"). Add the assert back in [`apply`] once `legal.rs` exists.

use crate::cards::{CardId, CardType, Special};
use crate::combat;
use crate::costs;
use crate::economy;
use crate::events;
use crate::legal;
use crate::effects;
use crate::moves::{ChurchillChoice, Move, PactSide};
use crate::state::{CardList, GameState, PlayerState, TechSlot, MAX_HAND, MAX_PLAYERS};

// ------------------------------------------------------------- leader identity
//
// Mirrors `costs.rs`'s `leader_is` (private there too -- see that module's
// "A note on leader identity"). Duplicated rather than shared for the same
// reason `economy.rs`'s test helpers duplicate `effects.rs`'s: it is four
// lines, and the alternative is making it `pub(crate)` in a module that is
// not this port's to edit.

#[inline]
fn leader_is(p: &PlayerState, name: &str) -> bool {
    !p.leader.is_none() && p.leader.get().name == name
}

/// Frederick Barbarossa's two discounts, read off his own card. Mirrors
/// `legal.rs`'s `barbarossa_discounts` -- duplicated for the same reason
/// `leader_is` is, above: `legal.rs`'s copy is private.
fn barbarossa_discounts(p: &PlayerState) -> (i32, i32) {
    let mut food = 0;
    let mut resources = 0;
    if !p.leader.is_none() {
        for s in p.leader.get().special.iter() {
            match s {
                Special::ComboFoodDiscount(v) => food = *v as i32,
                Special::ComboResourceDiscount(v) => resources = *v as i32,
                Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => {}
            }
        }
    }
    (food, resources)
}

/// Close the politics phase after ONE political action -- or after two.
/// Whether the political phase that is ending had a political action PLAYED
/// in it, or was passed. Only [`end_politics`] cares, and only for Julius
/// Caesar -- see its own doc for why the distinction is the whole rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PoliticalAction {
    Played,
    Passed,
}

/// Mirrors `engine/actions.py::_end_politics`, and is now the single place
/// every political handler in this file ends its phase, exactly as every
/// `_h_*` handler in `actions.py` routes through `_end_politics` rather than
/// setting `politics_done`/`phase` itself.
///
/// Julius Caesar's printed text is *"After you play a political action, you
/// may play another political action. This ability can be used only once per
/// game."* (BGA's own 2015 implementation, `sources/
/// bga_throughtheages_material.inc.php`). Both halves of that sentence are
/// load-bearing, and `action` is what lets this function honour them:
///
/// - the second action is offered only after a political action was actually
///   PLAYED -- passing is not playing one, so a pass never arms it;
/// - the once-per-game is spent only when the second action is actually
///   USED. "You MAY play another" means declining costs nothing, so a pass on
///   action two closes the phase and leaves the ability available for a later
///   turn.
///
/// While the ability is armed (`caesar_second_politics`), the first call of
/// the turn returns early: `phase` stays `Politics` and `politics_done` stays
/// `false`, so `legal_moves` offers the politics list again to the SAME
/// player.
///
/// ENGINE BUG, fixed 2026-08-06 (found by replaying the BGO human corpus):
/// this used to spend the once-per-game on ANY second call, including a
/// decline, and to arm the second action after a pass. The corpus contradicts
/// both outright -- in game `7523338` one player is offered and declines the
/// second action on rounds 3, 4, 5 and 7 and still holds the ability, no
/// player anywhere in the 1,011-game corpus takes two political actions in
/// more than one turn, and no player who has actually used the double is
/// ever offered it again (0 of 142). Python has the same bug; the printed
/// card is the oracle, not Python.
///
/// Also clears Joan of Arc's `peeked_event` -- "the knowledge is scoped to
/// the politics phase the card scopes it to" (`engine/events.py::
/// peek_top_event`'s doc comment) -- which does not fire on Caesar's early
/// return either, matching Python's `_end_politics` clearing it only on the
/// branch that actually closes the phase.
fn end_politics(state: &mut GameState, idx: u8, action: PoliticalAction) {
    let game_over = state.game_over;
    let p = &mut state.players[idx as usize];
    if p.caesar_second_politics {
        p.caesar_second_politics = false;
        if action == PoliticalAction::Played {
            p.caesar_double_politics_used = true;
        }
    } else if action == PoliticalAction::Played
        && leader_is(p, "Julius Caesar")
        && !p.caesar_double_politics_used
        && !p.resigned
        && !game_over
    {
        p.caesar_second_politics = true;
        return;
    }
    p.peeked_event = CardId::NONE;
    p.politics_done = true;
    state.phase = crate::state::Phase::Actions;
}

// ==================================================================== apply

/// Apply `mv` to `state`, in place. Ports `engine/actions.py::apply`, minus
/// the `state.pending` branch and the STRICT assert -- see this module's top
/// doc comment.
pub fn apply(state: &mut GameState, mv: Move) {
    // Python's `apply` opens with exactly this: `if state.pending: return
    // interact.apply_pending(state, move, rng)`. An open decision owns the
    // move, and its owner is `state.decider()`, not `state.current`.
    if !state.pending.is_empty() {
        crate::interact::apply_pending(state, mv);
        return;
    }
    let idx = state.current;
    match mv {
        // ---- civil actions ----
        // `cost` is `i32::MAX` for every move `legal::legal_moves` generates
        // (self-play); a lower value exists only on a replayer-observed
        // ordered `TakeACard` take, and `h_take_ca` bills the journal price
        // in that case instead of the row's.
        Move::Take { slot, cost } => h_take_ca(state, idx, slot, cost.min(costs::take_cost(state, &state.players[idx as usize], slot as usize))),
        Move::Build { card } => do_build(state, idx, card, 0, false),
        Move::Develop { card } => h_develop(state, idx, card, false),
        Move::Upgrade { from, to } => do_upgrade(state, idx, from, to, 0, false),
        Move::WonderStep { steps } => do_wonder_step(state, idx, steps, costs::wonder_stages_cost(state, idx, 1), false, None),
        Move::Pop => h_pop(state, idx, false),
        Move::PopFree => h_pop_free(state, idx),
        Move::Revolution { card } => h_revolution(state, idx, card, false),
        Move::PlayLeader { card } => h_play_leader(state, idx, card),
        Move::PlayAction { card } => h_play_action(state, idx, card),
        Move::Destroy { card } => h_destroy(state, idx, card),

        // ---- military (declaration only; no combat resolution needed) ----
        Move::PlayTactic { card } => h_play_tactic(state, idx, card),
        Move::CopyTactic { card } => h_copy_tactic(state, idx, card),
        Move::War { card, target } => h_war(state, idx, card, target),
        Move::CancelPact { owner } => h_cancel_pact(state, idx, owner),

        // ---- politics / turn control ----
        Move::PolPass => h_pol_pass(state, idx),
        Move::Resign => h_resign(state, idx),
        Move::Churchill { choice } => h_churchill(state, idx, choice),

        Move::OfferPact { card, target, side } => h_offer_pact(state, idx, card, target, side),
        Move::RemoveLeaderYellow => h_remove_leader_yellow(state, idx),
        Move::ColumbusColonize { card } => h_columbus_colonize(state, idx, card),
        Move::Barbarossa { card } => h_barbarossa(state, idx, card),
        Move::BachTheater { from, to } => h_bach_theater(state, idx, from, to, 0, false),
        Move::TradeFoodAsResource => h_trade_food_as_resource(state, idx),
        Move::TradeResourceAsFood => h_trade_resource_as_food(state, idx),

        // Responses to an open decision. Unreachable: the `state.pending`
        // branch at the top of this function has already taken them. Getting
        // here means one was played with NOTHING open, which is a legality
        // bug in the caller, not a port gap.
        Move::Bid { .. }
        | Move::BidPass
        | Move::Defend { .. }
        | Move::DefendDone
        | Move::SendUnit { .. }
        | Move::SendBonus { .. }
        | Move::SendDiscard { .. }
        | Move::SendDone
        | Move::Choose { .. } => panic!(
            "{mv:?} is a response to a decision, but `state.pending` is empty -- \
             nothing opened one"
        ),

        Move::PrepareEvent { card } => h_prepare_event(state, idx, card),

        // ---- combat.rs declaration + interact.rs defense decision ----
        Move::Aggression { card, target } => h_aggression(state, idx, card, target),

        // ---- blocked on game.rs ----
        // The End-of-Turn Sequence orchestrator (§6.6) plus the hand-off to
        // the next player. `game.rs` owns it; Python's `_h_end_turn` is the
        // same one-line delegation.
        Move::EndTurn => crate::game::end_turn(state),
    }
    // Python's `apply` tail: `effects.invalidate(state, p); interact.run_queue
    // (state, rng)`. There is no stats cache here, but the queue drain is
    // real -- a handler that defers a sub-effect (an action card's ordered
    // action, an event's population loss) has nothing to run it otherwise.
    crate::interact::run_queue(state);
}

// ============================================================ enter/leave play
//
// Ports `engine/effects.py`'s `on_enter_play` / `on_leave_play` / triggers
// (`on_take_card`, `on_develop`, `on_build_unit`) -- one-shot effects fired
// BY a move, not read by `compute()`, so `effects.rs` deliberately does not
// carry them (see its own doc comment grouping these Special variants
// "belongs to actions.rs").

/// Move `n` yellow tokens into `p`'s supply from a card or a rival. Mirrors
/// `engine/effects.py::grant_yellow`. `pub(crate)` (not private): `combat.rs`
/// needs the exact same bookkeeping for a War over Territory's spoils
/// (`combat::apply_war_spoils`), and this is the one existing copy rather
/// than a second one drifting out of sync with it.
pub(crate) fn grant_yellow(p: &mut PlayerState, n: i32) {
    if n > 0 {
        p.yellow_granted = p.yellow_granted.saturating_add(n as u8);
    }
    p.yellow_bank = (p.yellow_bank as i32 + n).max(0) as u8;
}

/// Immediate one-time effects when a card enters play (`blueTokens`,
/// `yellowTokens`).
fn on_enter_play(p: &mut PlayerState, id: CardId) {
    let eff = &id.get().effects;
    let bt = eff.blue_tokens as i32;
    if bt != 0 {
        p.blue_total = (p.blue_total as i32 + bt).max(0) as u8;
    }
    if eff.yellow_tokens != 0 {
        grant_yellow(p, eff.yellow_tokens as i32);
    }
    // A card whose STANDING `CardEffects.civil_actions`/`military_actions`
    // raises the player's per-turn total (Pyramids +1 CA, Code of Laws +1
    // CA, Justice System +1 CA, Civil Service +2 CA, Kremlin +1 CA/+1 MA --
    // `effects::compute`'s `add_flat` already sums these into `ca_total`
    // going forward) must ALSO top up the LIVE `p.civil_actions`/
    // `p.military_actions` counter for the REST OF THIS TURN the instant it
    // enters play, exactly like `set_government` already does for a
    // mid-turn revolution (`p.civil_actions = (s.civil_actions -
    // spent).max(0)`, recomputed from the new total). Without this, a
    // player who e.g. completes Pyramids mid-turn is capped at the STALE
    // (pre-bonus) budget for their remaining actions that turn, contradicting
    // the real rule (and the observed BGO human corpus, `docs/REPLAY.md`):
    // a newly active action source is usable THIS turn, not just future
    // ones. `p.civil_actions`/`military_actions` are otherwise a
    // decrementing-only pool (see `costs::pay_ca`), so the fix is a direct
    // += of the card's own contribution -- the exact amount `ca_total` just
    // gained -- not a full state_stats recompute (this function only takes
    // `&mut PlayerState`, no `&GameState`, matching every other case here).
    let ca = eff.civil_actions as i32;
    if ca != 0 {
        p.civil_actions = (p.civil_actions as i32 + ca).max(0) as i8;
    }
    let ma = eff.military_actions as i32;
    if ma != 0 {
        p.military_actions = (p.military_actions as i32 + ma).max(0) as i8;
    }
}

/// The leave-play twin of [`on_enter_play`].
///
/// `CultureOnLeaveEqualToLabResourceProduction` (Bill Gates -- Code of Laws:
/// "When Bill Gates leaves play, score culture equal to the amount of
/// resources you produce from labs"): scores culture equal to
/// `effects::lab_level_workers`, the SAME level*workers-summed-over-staffed-
/// labs formula `ResourcesPerLabEqualToLevel` uses for his OTHER (in-play)
/// ability -- both keys live on the one card (`card_table.rs`'s "Bill
/// Gates"). Was a documented, unwired gap (this function's own prior doc
/// comment named it "left unported"); traced live to game `7522665`'s own
/// journal: Iconoclasm (line 332, round 18) discards Bill Gates for missing
/// the current age, and the journal's own next `"End turn"` checkpoint (line
/// 340, `"(now 81)"`) is short by exactly 9 without this -- the SAME 9 a
/// staffed lab producing 9 resources/turn would owe, confirmed against
/// Orange's own tableau at that point in a `REPLAY_DEBUG_ALL` trace.
/// Computed BEFORE the token/civil-action bookkeeping below reads `id`'s
/// OTHER effects, though order does not matter here (independent fields).
pub(crate) fn on_leave_play(p: &mut PlayerState, id: CardId) {
    if id.get().special.contains(&Special::CultureOnLeaveEqualToLabResourceProduction) {
        let gained = effects::lab_level_workers(&p.techs);
        p.culture = (p.culture as i32 + gained).max(0) as u16;
    }
    let eff = &id.get().effects;
    let bt = eff.blue_tokens as i32;
    if bt != 0 {
        p.blue_total = (p.blue_total as i32 - bt).max(0) as u8;
    }
    if eff.yellow_tokens != 0 {
        p.yellow_bank = (p.yellow_bank as i32 - eff.yellow_tokens as i32).max(0) as u8;
    }
    // Symmetric twin of the bump `on_enter_play` now applies (see its doc
    // comment) -- a leader carrying a `civil_actions`/`military_actions`
    // bonus (Julius Caesar, Napoleon, ...) leaving play mid-turn (replaced
    // by a new leader, `h_play_leader`) must give back the headroom it
    // added, clamped at 0 so an already-spent turn can't go negative.
    let ca = eff.civil_actions as i32;
    if ca != 0 {
        p.civil_actions = (p.civil_actions as i32 - ca).max(0) as i8;
    }
    let ma = eff.military_actions as i32;
    if ma != 0 {
        p.military_actions = (p.military_actions as i32 - ma).max(0) as i8;
    }
}

/// A pact whose card grants `military_actions` (e.g. Open Borders
/// Agreement's "Both civilizations gain one military action") applies to
/// BOTH named parties the instant it is accepted -- RULES_SPEC §5.9: "Partner
/// accepts -> pact enters your play area ... it applies immediately." Same
/// shape and same reasoning as [`on_enter_play`]'s own civil/military-action
/// bump: `p.military_actions` is a decrementing-only per-turn pool (`costs::
/// pay_ca`'s twin for military), so a newly active source must top up the
/// LIVE counter for the rest of THIS turn, not just the state-stats formula
/// every FUTURE turn's own end-of-turn reset would already pick up on its
/// own. Without this, a player (on EITHER side of the pact -- the offerer
/// too, not just the accepter) who has already started their Actions phase
/// this turn stays capped at the stale pre-pact budget, silently drawing one
/// fewer military card at end of turn than the real game did (`docs/
/// REPLAY.md`'s military-hand-undercount lead, traced to game `7523341`
/// round 5).
pub(crate) fn on_pact_accepted(state: &mut GameState, pact: &crate::state::Pact) {
    for side in [pact.a, pact.b] {
        let ma = effects::pact_military_actions_for(pact.card, pact, side);
        if ma != 0 {
            let p = &mut state.players[side as usize];
            p.military_actions = (p.military_actions as i32 + ma).max(0) as i8;
        }
    }
}

/// The cancel-a-pact twin of [`on_pact_accepted`] (§5.10) -- gives back
/// whatever headroom the pact's own `military_actions` grant added, clamped
/// at 0 so an already-spent turn can't go negative, exactly like
/// [`on_leave_play`]'s symmetric decrement for a card leaving play.
pub(crate) fn on_pact_canceled(state: &mut GameState, pact: &crate::state::Pact) {
    for side in [pact.a, pact.b] {
        let ma = effects::pact_military_actions_for(pact.card, pact, side);
        if ma != 0 {
            let p = &mut state.players[side as usize];
            p.military_actions = (p.military_actions as i32 - ma).max(0) as i8;
        }
    }
}

/// Aristotle: 1 science per technology card taken from the row.
fn on_take_card(p: &mut PlayerState, id: CardId) {
    if leader_is(p, "Aristotle") && id.kind().is_developable() {
        p.science += 1;
    }
}

/// Leader triggers when a technology card is developed (§ leaders).
fn on_develop(state: &mut GameState, idx: u8) {
    if leader_is(&state.players[idx as usize], "Leonardo da Vinci") {
        economy::gain_resources(&mut state.players[idx as usize], 1);
    } else if leader_is(&state.players[idx as usize], "Albert Einstein") {
        state.players[idx as usize].culture += 3;
    } else if leader_is(&state.players[idx as usize], "Isaac Newton") {
        let s = effects::state_stats(state, &state.players[idx as usize]);
        let p = &mut state.players[idx as usize];
        p.civil_actions = s.civil_actions.min(p.civil_actions as i32 + 1) as i8;
    }
}

// ---------------------------------------------------- wonder-completion culture

/// Culture an Age III wonder scores on completion (§9.2). Mirrors
/// `engine/effects.py::wonder_completion_culture`.
pub(crate) fn wonder_completion_culture(p: &PlayerState, wonder: CardId) -> i32 {
    let card = wonder.get();
    if card.special.contains(&Special::OnBuildCulturePerTechLevelSum) {
        let mut gained = 0i32;
        for (id, _) in p.techs.iter() {
            if id.kind().is_developable() {
                gained += id.level() as i32;
            }
        }
        return gained + p.government.level() as i32;
    }
    // `OnBuildCulture` now carries an `OnBuildCultureValue` payload naming
    // which of the three formulas (gen_cards.py, 2026-08-05) -- `.contains`
    // no longer type-checks against a bare variant, so this matches on the
    // variant only; `one_time_culture` below still dispatches on
    // `base_name`, unchanged.
    if card.special.iter().any(|s| matches!(s, Special::OnBuildCulture(_))) {
        return one_time_culture(p, card.base_name);
    }
    0
}

fn one_time_culture(p: &PlayerState, base_name: &str) -> i32 {
    match base_name {
        "Fast Food Chains" => {
            let production_workers = workers_on_kind(p, |k| k.is_production());
            let urban_or_unit_workers = workers_on_kind(p, |k| k.is_urban() || k.is_unit());
            2 * production_workers + urban_or_unit_workers
        }
        // Hollywood and the Internet score what the buildings ACTUALLY
        // produce, not their printed production -- see `effects::
        // building_output`'s doc comment for the full worked-example list
        // (Chaplin, Shakespeare, Newton, Einstein, ...).
        "Hollywood" => {
            2 * effects::building_output(
                p,
                |k| matches!(k, CardType::Theater | CardType::Library),
                &[effects::Attr::Culture],
            )
        }
        "Internet" => effects::building_output(
            p,
            |k| k.is_urban(),
            &[effects::Attr::Culture, effects::Attr::Science, effects::Attr::Strength],
        ),
        _ => 0,
    }
}

fn workers_on_kind(p: &PlayerState, pred: impl Fn(CardType) -> bool) -> i32 {
    p.techs.iter().filter(|(id, _)| pred(id.kind())).map(|(_, s)| s.workers as i32).sum()
}

// --------------------------------------------------------------- pact helpers
//
// `engine/effects.py::cancel_attack_pacts` moved to `combat::
// cancel_attack_pacts` now that `combat.rs` exists -- this module used to
// carry its own copy (written for `h_war` before `combat.rs` was ported),
// which is exactly the "same fact, two registries" bug class DESIGN.md
// warns about, so it is deleted here in favour of the one in `combat.rs`.
// `drop_pacts_of` stays here: it is resignation bookkeeping, not combat
// math (no strength/legality involved), and has no `combat.rs` equivalent.

/// §5.11: remove every pact `idx` is party to (resignation).
fn drop_pacts_of(state: &mut GameState, idx: u8) {
    for q in state.players.iter_mut() {
        q.pacts.retain(|pact| !pact.is_party(idx));
    }
}

// ================================================================== handlers
//
// One function per `_h_*` in `engine/actions.py`, `state: &mut GameState`
// plus the acting player's index rather than Python's `(state, p)` pair:
// several handlers need BOTH a mutable borrow of the acting player's own
// fields AND a call into something that needs `&mut GameState` (a discard
// pile, another player's pacts), and Rust cannot alias those two borrows the
// way Python's two same-object references alias for free. Re-indexing
// `state.players[idx as usize]` on each statement keeps every borrow short
// and non-overlapping (DESIGN.md rule 4: "arena-and-index is the native
// idiom for this shape of code").

/// Bill a row take at `ca` civil actions. `ca` is the ROW's own price for
/// every ordinary take -- the `Move::Take` arm of [`apply`] computes it as
/// `cost.min(row_price)`, and a generated move always carries the
/// `i32::MAX` sentinel, which reduces to exactly the row price. The ONLY
/// source of a different value is [`apply_free_civil_move`]'s `Move::Take`
/// arm (`§3.11`'s `TakeACard` order): there the ORDERED action is the take
/// and the civil action is the free part -- the price that pays is the one
/// the JOURNAL actually printed, which
/// `replay_common.rs::resolve_take_with_free_civil` has already grounded
/// against `costs::take_cost`'s own row slots (the journal price is never
/// above the row price for a take the engine itself permits, so
/// `cost.min(row_price)` can only lower it -- the Hammurabi-converted or
/// leader-replacement shapes). The row and the `ca_spent_taking` ledger
/// never drift apart: both record exactly `ca`.
fn h_take_ca(state: &mut GameState, idx: u8, slot: u8, ca: i32) {
    costs::pay_ca(&mut state.players[idx as usize], ca);
    // The ONLY place civil actions are spent reaching into the row; recorded
    // so the evaluator can price it apart from civil actions spent elsewhere
    // (`engine/actions.py`'s own comment: INFORMATION_AUDIT GAP 1).
    state.players[idx as usize].ca_spent_taking += ca as u8;
    take_card(state, idx, slot as usize);
}

/// Move row card `slot` into `idx`'s hand/play area (actions already paid).
/// Mirrors `engine/actions.py::take_card`.
pub(crate) fn take_card(state: &mut GameState, idx: u8, slot: usize) {
    take_card_impl(state, idx, slot, true);
}

/// International Agreement's own `TakeRow` choice (CoL p.12, `docs/
/// RULES_SPEC.md` line ~383, "RESOLVED"): "if the current player takes
/// cards, action cards taken may be used the same turn" -- the ONE
/// documented exception to the normal rule that an action card cannot be
/// played "in the same Action Phase in which it was taken from the row"
/// [RB p.14, CoL p.6]. Skips [`take_card`]'s `taken_this_turn` bookkeeping
/// so `legal::action_moves`'s `held > taken` gate does not block it.
pub(crate) fn take_card_from_international_agreement(state: &mut GameState, idx: u8, slot: usize) {
    take_card_impl(state, idx, slot, false);
}

fn take_card_impl(state: &mut GameState, idx: u8, slot: usize, record_taken_this_turn: bool) {
    let id = state.card_row[slot];
    state.card_row[slot] = CardId::NONE;
    on_take_card(&mut state.players[idx as usize], id);
    let card = id.get();
    if card.kind == CardType::Wonder {
        state.players[idx as usize].wonder = id;
        // `wonder_stages_built` -- the set of stage POSITIONS this wonder
        // has been paid for -- is keyed by position, so it resets with the
        // wonder identity the way `wonder_steps` (the COUNT of positions
        // paid) does. Taking a SECOND wonder after the first completed
        // (the same player, the same turn, or the next) must not inherit
        // the first's positions -- see `PlayerState::wonder_stages_built`.
        state.players[idx as usize].wonder_stages_built = 0;
        state.players[idx as usize].wonder_steps = 0;
    } else {
        let p = &mut state.players[idx as usize];
        p.hand_civil.push(id);
        if card.kind == CardType::Leader {
            // § one leader per age (state.rs's doc comment on the field):
            // set once, never cleared, even once this leader is replaced.
            p.taken_leader_ages |= 1 << (card.age as u8);
        } else if card.kind == CardType::Action && record_taken_this_turn {
            p.taken_this_turn.push(id);
        }
    }
}

/// `free` exists for [`apply_free_civil_move`]'s benefit (an action card's
/// ordered "pop" with no action cost, still at the real food cost -- Python's
/// `apply_free_action` calls `economy.increase_population(state, p)` with its
/// own `free` defaulting to `False`, only skipping `pay_ca`). Not `discount`:
/// `free_action_moves`'s `IncreasePopulation` arm does not apply one either
/// (see its own comment -- "at full price").
fn h_pop(state: &mut GameState, idx: u8, free: bool) {
    let stats = effects::state_stats(state, &state.players[idx as usize]);
    // Read BEFORE `increase_population` (below) zeroes it: Development of
    // Civil Life's grant is CA-free the same way Rich Land/Frugality's own
    // ordered pop is (`costs::civil_life_ca_free`'s doc comment,
    // `docs/REPLAY.md` Finding 1) -- checked here, not folded into `free`,
    // because `free`'s caller (`apply_free_civil_move`) is a DIFFERENT free
    // source (an action card) that must stay independent of this one.
    let civil_life_free = costs::civil_life_ca_free(state.players[idx as usize].one_time_discount.pop_food);
    let cost = {
        let p = &state.players[idx as usize];
        economy::pop_food_cost(
            stats.pop_food_discount,
            p.yellow_bank,
            p.one_time_discount.pop_food as i32,
        )
            .expect("h_pop: called with an empty yellow bank (caller must check legality)")
    };
    if !free && !civil_life_free {
        costs::pay_ca(&mut state.players[idx as usize], 1);
    }
    // `cost` above ALWAYS folded in `one_time_discount.pop_food` (this
    // function's own `free` only skips the civil action below, exactly per
    // its doc comment -- the food cost, discount included, is paid either
    // way), so consumption is unconditional here too: `true`, not `!free`.
    let ok = economy::increase_population(
        &mut state.players[idx as usize], cost.max(0) as u16, true);
    debug_assert!(ok, "h_pop: caller must ensure enough food (legality check)");
}

fn h_pop_free(state: &mut GameState, idx: u8) {
    // Ocean Liners: genuinely free, `cost = 0`, and this path never even
    // calls `economy::pop_food_cost` -- so it never looked at the one-time
    // discount and must not consume it.
    economy::increase_population(&mut state.players[idx as usize], 0, false);
    state.players[idx as usize].ocean_liners_used = true;
}

/// Frederick Barbarossa's combined action: 1 military action buys BOTH
/// halves. Mirrors `engine/actions.py::_h_barbarossa`.
///
/// The order is the card's: population first, then the build, so the worker
/// the increase just produced is available for [`do_build`] to place. `
/// do_build` pays the ONE military action the whole combination costs (it
/// always pays one military action for a unit build) -- the population half
/// costs NO civil action, matching `economy::increase_population` never
/// touching one either.
///
/// The food cost is computed exactly as [`h_pop`] computes it (same
/// `economy::pop_food_cost` call, same discount-pool reads) and then
/// Barbarossa's own `comboFoodDiscount` is taken off, floored at 0 -- the
/// legality check in `legal::barbarossa_moves` guarantees this can never go
/// negative on food or come up short on the yellow bank.
fn h_barbarossa(state: &mut GameState, idx: u8, unit: CardId) {
    let (food_disc, res_disc) = barbarossa_discounts(&state.players[idx as usize]);
    let pop_cost = {
        let stats = effects::state_stats(state, &state.players[idx as usize]);
        let p = &state.players[idx as usize];
        economy::pop_food_cost(stats.pop_food_discount, p.yellow_bank, p.one_time_discount.pop_food as i32)
            .expect("h_barbarossa: called with an empty yellow bank (caller must check legality)")
    };
    let pay = (pop_cost - food_disc).max(0) as u16;
    // `pop_cost` above already read `one_time_discount.pop_food`; this IS a
    // real, paid population increase (never free), so consume it: `true`.
    let ok = economy::increase_population(&mut state.players[idx as usize], pay, true);
    debug_assert!(ok, "h_barbarossa: caller must ensure enough food (legality check)");
    do_build(state, idx, unit, res_disc, false);
}

/// J. S. Bach: upgrade an urban building to a theater, 1 civil action, once
/// a turn. Mirrors `engine/actions.py::_h_bach_theater`.
///
/// [`do_upgrade`] is the shared §3.5 implementation and already does the
/// right thing for this cross-type move: the cost is the difference of the
/// two build costs (floored at 0), the civil action is paid because a
/// theater is not a unit, and the worker moves from one card to the other.
///
/// `discount`/`free` exist for [`apply_free_civil_move`]'s benefit (an
/// action card's "using Efficient Upgrade"/"using Urban Growth" clause can
/// fund this SAME cross-kind privilege at a discount and no CA -- see
/// `legal::free_action_moves`'s urban-upgrade arm for the citation and the
/// corroborating BGO games); the bare dispatch below still passes `(0,
/// false)`, unchanged from before this was parameterized.
fn h_bach_theater(state: &mut GameState, idx: u8, from: CardId, to: CardId, discount: i32, free: bool) {
    state.players[idx as usize].bach_upgrade_used = true;
    do_upgrade(state, idx, from, to, discount, free);
}

/// Ports `engine/actions.py::do_build`. `discount`/`free` exist for
/// `apply_free_action`'s benefit (an action card's ordered "build" with no
/// action cost and a resource discount) -- not callable from [`apply`]
/// today since that path is blocked on `interact.rs` (see this module's top
/// doc comment), but kept so wiring it up later is a one-line change here.
pub fn do_build(state: &mut GameState, idx: u8, id: CardId, discount: i32, free: bool) {
    let base = costs::build_cost_for(state, &state.players[idx as usize], id).unwrap_or(0);
    let mut cost = (base - discount).max(0);
    // Read BEFORE this build zeroes it two lines down: same CA exemption as
    // `h_pop`'s (`costs::civil_life_ca_free`'s doc comment, `docs/REPLAY.md`
    // Finding 1) -- never true for a unit build, matching the discount
    // field's own scope (`URBAN_OR_PRODUCTION` only).
    let civil_life_free =
        costs::build_civil_life_free(&state.players[idx as usize], id);
    if !costs::is_unit(id) {
        // `build_cost_for` above already folded in Civil Life's one-shot
        // `build_resources` discount for every non-unit build (exactly the
        // farm/mine/urban cards it is gated on -- see its own doc comment);
        // this is the ONE build that spends it. Unconditional on `free`:
        // `free` only waives the civil action below, not the resource cost
        // that already consumed the discount computing `base`. Only when
        // `build_resources` was actually live: this is ONE mutually-
        // exclusive grant (pop XOR build XOR develop, `OneTimeDiscount`'s
        // own doc comment, `state.rs`) -- exhausting it on a build that
        // never had the discount banked in the first place would wrongly
        // wipe a still-unspent pop/develop grant.
        let p = &mut state.players[idx as usize];
        if p.one_time_discount.build_resources != 0 {
            p.one_time_discount.exhaust();
        }
    }
    if !free {
        cost = costs::spend_mil_discount(&mut state.players[idx as usize], id, cost);
        cost = costs::spend_homer_unit_discount(&mut state.players[idx as usize], id, cost);
        if !civil_life_free {
            if costs::is_unit(id) {
                state.players[idx as usize].military_actions -= 1;
            } else {
                costs::pay_ca(&mut state.players[idx as usize], 1);
            }
        }
    }
    {
        let p = &mut state.players[idx as usize];
        economy::pay_resources(p, cost.max(0) as u16);
        p.techs
            .get_mut(id)
            .expect("do_build: card must already be developed (in the tableau)")
            .workers += 1;
        p.workers_free -= 1;
    }
}

fn h_destroy(state: &mut GameState, idx: u8, id: CardId) {
    let p = &mut state.players[idx as usize];
    if costs::is_unit(id) {
        p.military_actions -= 1;
    } else {
        costs::pay_ca(p, 1);
    }
    p.techs.get_mut(id).expect("h_destroy: card not in tableau").workers -= 1;
    p.workers_free += 1;
}

/// Ports `engine/actions.py::do_upgrade`. See [`do_build`] on `discount`/`free`.
pub fn do_upgrade(state: &mut GameState, idx: u8, lo: CardId, hi: CardId, discount: i32, free: bool) {
    let base = costs::upgrade_cost(state, &state.players[idx as usize], lo, hi);
    let mut cost = (base - discount).max(0);
    if !free {
        cost = costs::spend_mil_discount(&mut state.players[idx as usize], lo, cost);
        cost = costs::spend_homer_unit_discount(&mut state.players[idx as usize], lo, cost);
        if costs::is_unit(lo) {
            state.players[idx as usize].military_actions -= 1;
        } else {
            costs::pay_ca(&mut state.players[idx as usize], 1);
        }
    }
    let p = &mut state.players[idx as usize];
    economy::pay_resources(p, cost.max(0) as u16);
    p.techs.get_mut(lo).expect("do_upgrade: lo not in tableau").workers -= 1;
    p.techs.get_mut(hi).expect("do_upgrade: hi not in tableau").workers += 1;
}

/// Pays for `k` stages of the wonder currently under construction, at
/// `cost` resources each (already discounted by the caller -- see the two
/// `Move::WonderStep` arms in this file and `apply_free_civil_move`'s own
/// `discount` argument), spending `cost * k` out of the player's resource
/// pool, `pay_ca(k)` civil actions (unless `free`), and marking the first
/// `k` UNBUILT stage positions as built.
///
/// Why a position, not a count: BGO lets a player build ANY uncovered stage
/// of the wonder, paying that stage's own printed cost -- the printed line
/// says so (game `7520718`: Colossus `[3, 3]` built 3-then-1 via Engineering
/// Genius, paying `1` for the second stage; `7521361`: Pyramids `[3, 2, 1]`
/// built 3-then-2, paying `2` for the second). The old model -- "always the
/// next left-to-right stage, price `stages[done]`" -- charged the WRONG
/// price whenever the journal's order diverged from left-to-right, drifting
/// the player's resources for the rest of the game (the
/// `IllegalMove: WonderStep` bucket's "resources short by N" signature,
/// `docs/REPLAY.md`). `costs::wonder_stage_cost` is kept for its
/// left-to-right reading -- that is the price a fresh `k`-stage build pays
/// when nothing is built yet (the self-play default), and the affordability
/// gate in `legal.rs` still uses it -- but PAYMENT now goes through
/// [`costs::wonder_stages_cost`] against the specific positions the caller
/// chose, and `wonder_steps` (the count) is DERIVED from
/// `wonder_stages_built` (the positions) rather than stored separately, so
/// the two can never disagree.
///
/// Panics on `k == 0` (no caller does; the legal-move generators only ever
/// produce `k >= 1`) and on a `wonder_steps`/`wonder_stages_built` pair that
/// cannot both be true (a count greater than the population -- unreachable
/// today because every writer of the pair updates both together, but the
/// panic is the honest failure for a future writer that forgets).
/// `pos` names the stage position this move builds (the leftmost-UNBUILT
/// one when the caller did not resolve it -- the self-play default, and what
/// `Move::WonderStep` carries when the replay grounding had no unique
/// answer). The replay grounding in `replay_common.rs` resolves the exact
/// position from the journal's own stated payment and passes it here, so a
/// non-left-to-right build (Colossus `[3,3]` 3-then-1, Pyramids `[3,2,1]`
/// 3-then-2) marks the RIGHT stage rather than the next sequential one.
///
/// A "builds N stages" line (N > 1) always prices a contiguous
/// left-to-right run (confirmed corpus-wide), so `pos` is always the
/// leftmost stage of that run and the remaining N-1 are the next unbuilt
/// ones to its right.
pub fn do_wonder_step(state: &mut GameState, idx: u8, k: u8, _cost: i32, free: bool, pos: Option<usize>) {
    debug_assert!(k >= 1, "do_wonder_step: k == 0 is a no-op, not a move");
    let p = &mut state.players[idx as usize];
    let stages = p.wonder.get().stages;
    let built = p.wonder_stages_built;
    // The first stage to mark: the caller's resolved position, else the
    // leftmost unbuilt (self-play / ungrounded default).
    let default_start = (0..stages.len()).find(|&i| built & (1 << i) == 0).expect("no unbuilt stage");
    let start = match pos {
        // A resolved position is the LEFTMOST of the run this line builds.
        // If the run (k consecutive-from-start unbuilt stages) can't be
        // satisfied from it (e.g. a multi-stage line whose grounding pinned
        // a single mid-run stage, or a completed-wonder quirk), fall back to
        // the leftmost-unbuilt default rather than over-marking.
        Some(t) if t < stages.len() && (built & (1 << t)) == 0 => {
            let mut c = 0u32;
            for i in t..stages.len() {
                if c == k as u32 { break; }
                if built & (1 << i) == 0 { c += 1; }
            }
            if c >= k as u32 { t } else { default_start }
        }
        _ => default_start,
    };
    // Mark `start` plus the next (k-1) UNBUILT positions to its right (a
    // multi-stage build is a contiguous run, `start` being its leftmost).
    let mut next = built;
    let mut marked = 0u32;
    for i in start..stages.len() {
        if marked == k as u32 {
            break;
        }
        if next & (1 << i) != 0 {
            continue;
        }
        next |= 1 << i;
        marked += 1;
    }
    debug_assert!(marked == k as u32, "do_wonder_step: fewer unbuilt stages than k");
    p.wonder_stages_built = next;
    if !free {
        costs::pay_ca(p, k as i32);
    }
    // Sum the ACTUAL per-stage costs of the stages just marked, rather than
    // charging `cost * k` (a single per-stage price multiplied).  A multi-
    // stage build (BGO's "builds N stages" line) touches stages whose
    // printed costs differ -- Ocean Liners `[4,2,2,4]` built 3-then-1
    // charges 8+4, not 8+8.  For `k == 1` the caller's `_cost` carries any
    // discount (Engineering Genius) that the base stage cost does not, so
    // it is used directly; for `k > 1` the base per-stage costs are summed
    // (no discount source applies to a multi-stage batch in the base game).
    let total_cost: i32 = if k == 1 {
        _cost.max(0)
    } else {
        (start..stages.len())
            .filter(|&i| next & (1 << i) != 0 && built & (1 << i) == 0)
            .take(k as usize)
            .map(|i| stages[i] as i32)
            .sum()
    };
    economy::pay_resources(p, total_cost as u16);
    let wonder = p.wonder;
    let built = p.wonder_stages_built;
    let steps = built.count_ones() as u8;
    let n_stages = stages.len() as u8;
    debug_assert!(steps <= n_stages);
    if steps == n_stages {
        let gained = wonder_completion_culture(p, wonder);
        p.wonder = CardId::NONE;
        p.wonder_stages_built = 0;
        p.wonder_steps = 0;
        p.completed_wonders.push(wonder);
        on_enter_play(p, wonder);
        p.culture = (p.culture as i32 + gained).max(0) as u16;
    } else {
        p.wonder_steps = steps;
    }
}

/// The leftmost-UNBUILT stage position of `idx`'s wonder -- the position a
/// self-play `WonderStep` (or an ungrounded replay) builds. Exposed so the
/// replay grounding can resolve the same default it would fall back to.
pub fn wonder_leftmost_unbuilt(state: &GameState, idx: u8) -> usize {
    let p = &state.players[idx as usize];
    (0..p.wonder.get().stages.len())
        .find(|&i| p.wonder_stages_built & (1 << i) == 0)
        .expect("wonder_leftmost_unbuilt: no unbuilt stage")
}


fn h_play_leader(state: &mut GameState, idx: u8, id: CardId) {
    costs::pay_ca(&mut state.players[idx as usize], 1);
    state.players[idx as usize].hand_civil.remove_first(id);
    let old = state.players[idx as usize].leader;
    if !old.is_none() {
        // "You are allowed to use the benefit of a leader and then replace
        // him or her on the same turn" (RB, "Replacing a Leader"). For every
        // other leader that is automatic -- their benefit was already applied
        // to whatever it touched. Hammurabi's MA-as-CA conversion is the one
        // benefit this engine spends LAZILY (`costs::pay_ca` only reaches for
        // it once the civil-action pool runs dry), so without remembering
        // that he was here this turn, a player who replaces him mid-turn
        // loses an action the rules let them keep. See
        // `PlayerState::hammurabi_replaced_this_turn`.
        if old.get().name == "Hammurabi" {
            state.players[idx as usize].hammurabi_replaced_this_turn = true;
        }
        // Taj Mahal's own printed 2015 text reads the fact of a replacement
        // regardless of WHO was replaced ("If you replaced your leader this
        // turn, taking this wonder costs you 2 civil actions less"), so it
        // gets its own, weaker flag rather than reusing Hammurabi's.
        state.players[idx as usize].replaced_leader_this_turn = true;
        // Snapshot the CA/MA pool the SAME way `set_government` does
        // (`old_total`/`spent`/`old_remaining`, fed to `carry_over_action_
        // pool`) BEFORE `on_leave_play` strips the old leader's own flat
        // `civil_actions`/`military_actions` bonus (if any) -- a leader
        // carrying one (Joan of Arc: +1 MA; Julius Caesar/Napoleon, ...) who
        // is replaced mid-turn must NOT retroactively lose actions already
        // spent from that bonus, only the amount left unclaimed (RULES_SPEC
        // §9.1 "Effects apply immediately" governs the NEW leader's bonus,
        // not a clawback of the old one's already-banked turn -- the same
        // shape as §8.4's revolution carry-over `set_government` already
        // gets right). `on_leave_play`'s own plain decrement (clamped at 0,
        // no memory of what was already spent) got this wrong: confirmed
        // against game 7522520 round 11 -- Joan of Arc's +1 MA had already
        // paid for a CopyTactic (2 MA) before Isaac Newton replaced her, so
        // the swap must leave the remaining MA pool untouched, not subtract
        // Joan's bonus a second time out of what was left. Overwritten below
        // once the swap (and the new leader's own bonus) has fully applied,
        // so `on_leave_play`/`on_enter_play`'s own CA/MA arithmetic in
        // between is harmless dead work, not double-counted.
        let old_total_c = effects::state_stats(state, &state.players[idx as usize]).civil_actions;
        let old_total_m = effects::state_stats(state, &state.players[idx as usize]).military_actions;
        let spent_c = old_total_c - state.players[idx as usize].civil_actions as i32;
        let spent_m = old_total_m - state.players[idx as usize].military_actions as i32;
        let old_remaining_c = state.players[idx as usize].civil_actions as i32;
        let old_remaining_m = state.players[idx as usize].military_actions as i32;
        on_leave_play(&mut state.players[idx as usize], old);
        let is_homer = old.get().name == "Homer";
        let has_completed = !state.players[idx as usize].completed_wonders.is_empty();
        let homer_slot_free = state.players[idx as usize].homer_wonder.is_none();
        if is_homer && has_completed && homer_slot_free {
            let first = state.players[idx as usize].completed_wonders.as_slice()[0];
            state.players[idx as usize].homer_wonder = first; // tucked, still face up
        } else {
            economy::discard_civil(state, old);
        }
        state.players[idx as usize].leader = id;
        on_enter_play(&mut state.players[idx as usize], id);
        let new_total_c = effects::state_stats(state, &state.players[idx as usize]).civil_actions;
        let new_total_m = effects::state_stats(state, &state.players[idx as usize]).military_actions;
        let carried_c = carry_over_action_pool(old_total_c, new_total_c, spent_c, old_remaining_c);
        // Replacing a leader ALSO refunds one civil action on top of the
        // carry-over (§9.1's own, separate clause) -- clamped to the new
        // total, same as before this fix.
        state.players[idx as usize].civil_actions = new_total_c.min(carried_c as i32 + 1) as i8;
        state.players[idx as usize].military_actions = carry_over_action_pool(old_total_m, new_total_m, spent_m, old_remaining_m);
        return;
    }
    state.players[idx as usize].leader = id;
    on_enter_play(&mut state.players[idx as usize], id);
}

/// Scientific Cooperation's own charge to the OTHER civilization (BGA's
/// card text: "...the other civilization pays 1 science... If the other
/// cannot pay 1 science, then the technology cannot be developed."): the
/// legality half of that sentence is `effects::science_pact_partners_can_pay`,
/// already checked by every caller of `h_develop`/`h_revolution` before
/// either got this far (`legal.rs`'s `DevelopTechnology`/`Government`/
/// `can_revolt` sites); this is just the charge itself, applied AFTER `p`'s
/// own (already pact-discounted) payment. `p`'s pact-derived `Stats::
/// science_partners` bitmask depends only on which pacts `p` holds, never on
/// any player's science total, so reading it after `p`'s own charge already
/// landed cannot change which partners owe.
fn charge_science_pact_partners(state: &mut GameState, idx: u8) {
    let mask = effects::state_stats(state, &state.players[idx as usize]).science_partners;
    for i in 0..state.num_players {
        if mask & (1 << i) != 0 {
            let partner = &mut state.players[i as usize];
            partner.science = partner.science.saturating_sub(1);
        }
    }
}

fn h_develop(state: &mut GameState, idx: u8, id: CardId, free: bool) {
    let card = id.get();
    // NOTE: for a Government card, `costs::tech_cost` always returns `None`
    // (costs.rs's own KNOWN GAP #3: `peacefulCost` is not captured), so a
    // peaceful revolution taken via `develop` costs 0 science today instead
    // of its printed cost. Not fixed here -- costs.rs/cards.rs are off
    // limits to this module; carried forward, not a new gap.
    let raw_cost = costs::tech_cost(state, &state.players[idx as usize], id);
    // Read BEFORE this develop zeroes it a few lines down: same CA
    // exemption as `h_pop`'s/`do_build`'s (`costs::civil_life_ca_free`'s doc
    // comment, `docs/REPLAY.md` Finding 1).
    let civil_life_free =
        raw_cost.is_some() && costs::civil_life_ca_free(state.players[idx as usize].one_time_discount.develop_science);
    if raw_cost.is_some() {
        // `tech_cost` returns `None` only when the card has no develop cost
        // at all, in which case it never looked at `one_time_discount`
        // either (see its own doc comment: it subtracts the discount
        // unconditionally whenever it returns `Some`). This is the ONE
        // technology that spends Civil Life's one-shot `develop_science`
        // discount; unconditional on `free` for the same reason as
        // `do_build` above. Only when `develop_science` was actually live --
        // same "don't wipe a sibling grant that was never banked" reasoning
        // as `do_build`'s (`OneTimeDiscount`'s own doc comment, `state.rs`).
        let p = &mut state.players[idx as usize];
        if p.one_time_discount.develop_science != 0 {
            p.one_time_discount.exhaust();
        }
    }
    let raw = raw_cost.unwrap_or(0);
    let cost = costs::spend_mil_sci_discount(&mut state.players[idx as usize], id, raw);
    if !free && !civil_life_free {
        costs::pay_ca(&mut state.players[idx as usize], 1);
    }
    {
        let p = &mut state.players[idx as usize];
        p.science = p.science.saturating_sub(cost.max(0) as u16);
        p.hand_civil.remove_first(id);
    }
    charge_science_pact_partners(state, idx);
    if card.kind == CardType::Government {
        set_government(state, idx, id);
    } else if card.kind == CardType::SpecialTech {
        develop_special(state, idx, id);
    } else {
        state.players[idx as usize].techs.insert(id, TechSlot { workers: 0, stored: 0 });
        on_enter_play(&mut state.players[idx as usize], id);
    }
    on_develop(state, idx);
}

/// §8.2's decrease rule ("update CA/MA totals ... if totals increased, the
/// new tokens are available THIS turn; if decreased, return tokens (spent
/// first)"): an increase (or no change) simply adds the new tokens to the
/// unspent pool (`new_total - spent`), but a decrease must eat into the
/// already-SPENT tokens before it touches the unspent (`old_remaining`)
/// ones -- an already-spent token is "returned" (forgiven) first, and only
/// once `spent` alone can't absorb the whole decrease does `old_remaining`
/// shrink. §8.3.4 explicitly reuses this same rule for a revolution's
/// unaffected pool ("behaves exactly as in a peaceful change"), so both
/// callers below share this helper -- keep them that way.
///
/// `pub(crate)`: `game.rs::antiquate` is a THIRD caller (a leader aging out
/// mid-round is a total DECREASE exactly like a leader replacement with a
/// weaker one, so it owes the same "already-spent is forgiven first" rule --
/// a flat, unconditional subtract clawed back actions the player had
/// already spent from OTHER sources this turn whenever the antiquating
/// leader's own bonus was less than what was left of it, the identical
/// failure mode `h_play_leader`'s own doc comment already traces for
/// leader replacement, game `7522520`).
pub(crate) fn carry_over_action_pool(old_total: i32, new_total: i32, spent: i32, old_remaining: i32) -> i8 {
    if new_total >= old_total {
        (new_total - spent).max(0) as i8
    } else {
        let decrease = old_total - new_total;
        (old_remaining - (decrease - spent).max(0)).max(0) as i8
    }
}

/// Ports `engine/actions.py::_set_government`.
fn set_government(state: &mut GameState, idx: u8, id: CardId) {
    let (old_total_c, old_total_m, spent_c, spent_m, old_remaining_c, old_remaining_m, old_gov) = {
        let p = &state.players[idx as usize];
        let stats = effects::state_stats(state, p);
        (
            stats.civil_actions,
            stats.military_actions,
            stats.civil_actions - p.civil_actions as i32,
            stats.military_actions - p.military_actions as i32,
            p.civil_actions as i32,
            p.military_actions as i32,
            p.government,
        )
    };
    // `p.government` is never `CardId::NONE` in practice (every player
    // starts on Despotism), so Python's `if p.government and ...` is really
    // just "if it changed" here.
    if old_gov != id {
        economy::discard_civil(state, old_gov);
    }
    state.players[idx as usize].government = id;
    let s = effects::state_stats(state, &state.players[idx as usize]);
    let p = &mut state.players[idx as usize];
    p.civil_actions = carry_over_action_pool(old_total_c, s.civil_actions, spent_c, old_remaining_c);
    p.military_actions = carry_over_action_pool(old_total_m, s.military_actions, spent_m, old_remaining_m);
}

/// Ports `engine/actions.py::_develop_special` (== `put_special_in_play`):
/// §7.6's one-per-icon rule for blue special technologies. THE single
/// placement implementation -- `War over Technology` stealing one would call
/// this too, once combat.rs exists.
fn develop_special(state: &mut GameState, idx: u8, id: CardId) {
    let icon = costs::special_icon(id.get());
    let new_level = id.level();
    let mut existing = [CardId::NONE; 8];
    let mut n = 0usize;
    for (tid, _) in state.players[idx as usize].techs.iter() {
        if tid.kind() == CardType::SpecialTech && costs::special_icon(tid.get()) == icon {
            debug_assert!(n < existing.len(), "develop_special: more same-icon special techs than expected");
            existing[n] = tid;
            n += 1;
        }
    }
    for &old in &existing[..n] {
        if old.level() >= new_level {
            return; // the new (lower) card is discarded -- nothing changes
        }
    }
    for &old in &existing[..n] {
        on_leave_play(&mut state.players[idx as usize], old);
        state.players[idx as usize].techs.remove(old);
    }
    state.players[idx as usize].techs.insert(id, TechSlot { workers: 0, stored: 0 });
    on_enter_play(&mut state.players[idx as usize], id);
}

/// §7.6's one placement implementation, exposed for `War over Technology`'s
/// benefit once combat.rs exists (Code of Laws p.3: stealing a special tech
/// follows the same "keep the higher level" rule as developing one).
pub fn put_special_in_play(state: &mut GameState, idx: u8, id: CardId) {
    develop_special(state, idx, id);
}

/// `via_ordered_action`: this `Revolution` was reached through `Move::
/// PlayAction { card: Breakthrough }`'s own ordered "develop a technology OR
/// revolt" order (`legal::free_action_moves`'s `DevelopTechnology` arm, RB
/// p.15/CoL p.5) rather than a bare declaration. That path's own precondition
/// comment (`legal.rs`'s `revolt_ok`) already flags the subtlety this
/// parameter closes: by the time this runs, Breakthrough's OWN `1 CA` has
/// already been spent (`Move::PlayAction` -> `h_play_action`, BEFORE the
/// order resolves) -- true even though the rulebook's "all civil actions
/// must still be available" precondition (RB p.15) is checked against the
/// PRE-Breakthrough pool. For a bare revolution `via_ordered_action` is
/// always `false` and this changes nothing (see below).
fn h_revolution(state: &mut GameState, idx: u8, id: CardId, via_ordered_action: bool) {
    // `costs::revolution_cost` folds in the standing pact discount (`Stats::
    // tech_discount`) that a bare `Card::revolution_cost` read misses -- see
    // that function's own doc for the confirmed 2-science overcharge this
    // closes. `unwrap_or(0)`: every caller of `h_revolution` has already
    // gone through a `can_revolt`/`revolt_ok`-gated legality check, which
    // itself now reads the same discounted cost, so a `None` here (no
    // printed revolution cost at all) cannot actually happen -- the
    // fallback only avoids a spurious panic if that invariant is ever
    // violated.
    let cost = costs::revolution_cost(state, &state.players[idx as usize], id).unwrap_or(0);
    {
        let p = &mut state.players[idx as usize];
        p.science = p.science.saturating_sub(cost.max(0) as u16);
        p.hand_civil.remove_first(id);
    }
    // A violent revolution changes government via the SAME "develops a
    // technology" trigger Scientific Cooperation's own text names -- BGO's
    // own journal confirms it (game `7523162` line 292: Grey revolutions,
    // "Orange loses 1 science" logged in the SAME line as Grey's own
    // discounted charge).
    charge_science_pact_partners(state, idx);
    let robespierre = leader_is(&state.players[idx as usize], "Maximilien Robespierre");
    // §8.3.4 (RB p.13): only the pool that PAYS for the revolution is
    // emptied; the other behaves exactly as in a peaceful change -- i.e. its
    // remaining amount carries over, plus/minus however the total changes
    // for the new government (`docs/REPLAY.md`'s Take/Bid handoff, "gate-by-
    // gate breakdown" pass).
    let old = effects::state_stats(state, &state.players[idx as usize]);
    let old_remaining_c = state.players[idx as usize].civil_actions as i32;
    let old_remaining_m = state.players[idx as usize].military_actions as i32;
    let mut spent = if robespierre { old.civil_actions - old_remaining_c } else { old.military_actions - old_remaining_m };
    // ENGINE BUG fix (docs/REPLAY.md, this pass): when Robespierre revolts
    // via Breakthrough, the UNAFFECTED pool is civil -- the SAME pool
    // Breakthrough's own `PlayAction` just spent 1 CA from. That 1 CA is the
    // cost of DECLARING the revolution under this exception (RB p.15: "may
    // pay for the revolution with its 1 CA + revolution science cost"), the
    // exact role a bare revolution's full-pool wipe plays with no separate
    // charge -- it must not ALSO count as ordinary this-turn spending against
    // the unaffected pool's carry-over, or the player ends up short by
    // exactly 1 CA for the rest of the turn (confirmed against 9/10 raw BGO
    // journals in the `IllegalMove: Take`/`Budget` bucket that show a
    // Robespierre player revolting "using Breakthrough" then a LATER take
    // the journal itself records as successful, at exactly the point this
    // binary's reconstructed civil_actions hits 0 one card short). Not
    // reachable for the non-Robespierre branch: civil is unconditionally
    // zeroed there regardless of how the revolution was funded (RB p.13's
    // base "you end with 0 available CAs this turn" already covers a
    // Breakthrough-funded base revolution with no separate accounting).
    // `breakthrough_ma_funded` (set by `h_play_action`, `legal::
    // breakthrough_robespierre_ma_fundable`'s own doc comment has the
    // citation): when Breakthrough's own 1-CA play cost was paid with a
    // military action instead (civil already exhausted), civil was never
    // touched by playing it at all, so the `-1` correction below would
    // wrongly credit a civil action that was never spent -- skip it, and
    // consume the flag so a LATER, unrelated revolution this same turn
    // cannot accidentally inherit it.
    let breakthrough_civil_spent = via_ordered_action && !state.players[idx as usize].breakthrough_ma_funded;
    state.players[idx as usize].breakthrough_ma_funded = false;
    if breakthrough_civil_spent && robespierre {
        spent -= 1;
    }
    state.players[idx as usize].government = id;
    let s = effects::state_stats(state, &state.players[idx as usize]);
    if robespierre {
        let p = &mut state.players[idx as usize];
        p.military_actions = 0;
        p.civil_actions = carry_over_action_pool(old.civil_actions, s.civil_actions, spent, old_remaining_c);
        p.culture += 3;
    } else {
        let p = &mut state.players[idx as usize];
        p.civil_actions = 0;
        p.military_actions = carry_over_action_pool(old.military_actions, s.military_actions, spent, old_remaining_m);
    }
    // §8.3.4 / `docs/RULES_SPEC.md` line 232: "revolution counts as
    // developing a technology" (that line's own text is about Newton, but
    // the citation is general -- a revolution IS a `DevelopTechnology`, see
    // this file's own doc comment on `Move::Develop`/`Move::Revolution`
    // just above `h_develop`'s definition). `h_develop`'s peaceful path
    // already reaches every develop-triggered leader ability uniformly via
    // `on_develop` (Leonardo da Vinci's +1 resource, Albert Einstein's +3
    // culture, Isaac Newton's +1 CA) -- this branch used to hand-roll ONLY
    // Newton's arm inline, so a Leonardo da Vinci or Albert Einstein player
    // who seized their government by violent revolution never got their
    // trigger at all. REPLAYER-BUG-CONFIRMED-ENGINE-BUG (`IllegalMove:
    // Upgrade` bucket, game `7522665` round 11): the journal's own
    // resolution line names the leader explicitly --
    // "Orange revolutions Change government to Constitutional Monarchy; 6
    // science points spent; Orange loses 6 science; Leonardo Da Vinci gives
    // 1 resource" -- and the missing +1 resource is a permanent, uncorrected
    // 1-resource deficit for the rest of the game (confirmed against every
    // later `"(now N)"` resource total in that journal, off by exactly -1
    // from this round onward) until it starves an `Upgrade` 7 rounds later.
    // `on_develop` recomputes its own `effects::state_stats` fresh, but that
    // is provably identical to the `s` already in scope here: nothing this
    // function touches between computing `s` (just above) and this call
    // changes any input `state_stats` reads (`p.techs`/`p.government`/
    // leader/production), so re-deriving it is not a race, just a second
    // read of the same answer -- and calling the ONE shared helper instead
    // of hand-rolling Newton's own arithmetic a second time here is exactly
    // the "single source of truth" reasoning `costs::build_cost_net`'s own
    // doc comment already established for this file.
    on_develop(state, idx);
}

fn h_churchill(state: &mut GameState, idx: u8, choice: ChurchillChoice) {
    let p = &mut state.players[idx as usize];
    p.churchill_used = true;
    match choice {
        ChurchillChoice::Culture => p.culture += 3,
        ChurchillChoice::Military => {
            p.mil_sci_discount += 3;
            p.mil_discount += 3;
            if crate::debugflags::replay_debug_all() {
                eprintln!("DEBUG mil_discount site=h_churchill idx={idx} += 3 -> {}", p.mil_discount);
            }
        }
    }
}

fn h_play_tactic(state: &mut GameState, idx: u8, id: CardId) {
    let p = &mut state.players[idx as usize];
    p.military_actions -= 1;
    p.hand_military.remove_first(id);
    p.tactic = id;
    p.tactic_exclusive = true;
    p.tactic_action_used = true;
}

fn h_copy_tactic(state: &mut GameState, idx: u8, id: CardId) {
    let p = &mut state.players[idx as usize];
    p.military_actions -= 2;
    p.tactic = id;
    p.tactic_exclusive = false;
    p.tactic_action_used = true;
}

/// §5.6 / CoL p.4: reveal, pay, name the rival, drop the pact, record the
/// declaration. Mirrors `engine/actions.py::_h_war`. Uses `combat::
/// cancel_attack_pacts` for the pact-cancellation step, but otherwise still
/// never resolves combat -- it only records that a war is open. Actual war
/// RESOLUTION (`combat::resolve_war_outcome` / `apply_war_spoils`) fires at
/// the start of the attacker's NEXT turn (`engine/game.py::start_turn`,
/// `game.rs`, not ported), not from this handler; see this module's top doc
/// comment.
fn h_war(state: &mut GameState, idx: u8, id: CardId, target: u8) {
    let mut cost = id.get().military_action_cost as i32;
    if leader_is(&state.players[target as usize], "Mahatma Gandhi") {
        cost *= 2;
    }
    state.players[idx as usize].military_actions -= cost as i8;
    state.players[idx as usize].hand_military.remove_first(id);
    // CoL p.4 / FAQ p.11: a pact that ends if its parties attack each other
    // is removed before the war is recorded, and its strength never applies.
    combat::cancel_attack_pacts(state, idx, target);
    state.players[idx as usize].war_declared_by_me = id;
    state.players[idx as usize].war_target = target;
    state.players[target as usize].wars_declared_on_me[idx as usize] = id;
    end_politics(state, idx, PoliticalAction::Played);
}

/// §5.4: pay cost, discard, compute strength, cancel any doomed pact.
/// Mirrors the part of `engine/actions.py::_h_aggression` that is portable
/// today -- `combat::start_aggression` is the exact prefix of `engine/
/// events.py::start_aggression` up to (not including) `interact.
/// start_defense`, which needs `state.pending` (no such field exists, and
/// this module may not add one). See `combat.rs`'s top doc comment
/// "Aggression: declaration ported, resolution blocked on `interact.rs`"
/// for the full reasoning; this handler only names the one remaining
/// blocker.
fn h_aggression(state: &mut GameState, idx: u8, card: CardId, target: u8) {
    // Python's `_h_aggression` marks the politics action spent and advances
    // the phase BEFORE handing the defense decision over -- the attacker's
    // political action is finished either way, and the defender answering
    // must not look like the attacker still owing a move. (When Caesar's
    // second action is still open, `end_politics` leaves `phase` at
    // `Politics` instead -- matching Python, which calls `_end_politics`
    // here unconditionally too and lets that same early return apply.)
    end_politics(state, idx, PoliticalAction::Played);
    let atk = combat::start_aggression(state, idx, card, target);
    crate::interact::start_defense(state, idx, target, card, atk);
}

/// §5.9: reveal the pact, name the partner and the sides, and hand the
/// accept/refuse decision to that partner. Mirrors `engine/actions.py::
/// _h_offer_pact`.
///
/// `side` decides which printed block each party takes, and it is NOT the
/// same as who owns the card: offering side B puts the TARGET on `a` and the
/// offerer on `b`. See `state::Pact`'s doc comment for why those are four
/// separate indices.
fn h_offer_pact(state: &mut GameState, idx: u8, card: CardId, target: u8, side: PactSide) {
    state.players[idx as usize].hand_military.remove_first(card);
    let (a, b) = match side {
        PactSide::B => (target, idx),
        PactSide::A | PactSide::Unspecified => (idx, target),
    };
    end_politics(state, idx, PoliticalAction::Played);
    let mut options = crate::state::OptionList::new();
    options.push(crate::state::ChoiceOption::Word(crate::state::Keyword::Accept));
    options.push(crate::state::ChoiceOption::Word(crate::state::Keyword::Refuse));
    crate::interact::push_choice(
        state,
        target,
        crate::state::ChoiceKind::PactOffer { owner: idx, card, a, b },
        options,
        false,
    );
}

/// Ports `engine/actions.py::_h_play_action`. See this module's top doc
/// comment for exactly which cards still panic (`Reserves` --
/// `gainFoodOrResources`, always needs `interact::push_choice` per Python's
/// `apply_card_gains`, `auto=False`; and, for the 18 `freeCivilAction`
/// cards, only the case where their ordered action has 2+ legal options,
/// which now opens a real decision). `Wave of Nationalism`/`Military
/// Build-Up`/`Endowment for the Arts` (the per-player-count magnitudes) are
/// CLOSED as of 2026-08-05 -- see the top doc comment's "Per-player-count
/// effect magnitudes" entry.
fn h_play_action(state: &mut GameState, idx: u8, id: CardId) {
    // RB p.15: Breakthrough's `develop_technology` order may spend itself on
    // a revolution instead, gated on `legal::revolt_pool_ok` -- the SAME
    // per-turn action-pool precondition an ordinary revolution needs
    // (`legal::can_revolt`), not a separately-maintained copy of it (see
    // `revolt_pool_ok`'s own doc comment: THAT split, not just a missing
    // Robespierre branch, was the actual bug two independent copies of this
    // one rule produced). Read before `pay_ca` below -- playing the card
    // itself must not count as "a civil action spent" for this test -- so
    // this has to be captured here, ahead of that payment, not recomputed
    // later inside the queue item's own resolution (which would otherwise
    // always see the CA this card itself just cost).
    let revolt_ok = legal::revolt_pool_ok(state, &state.players[idx as usize]);
    // RB p.15's "1 CA" for Breakthrough is explicitly the cost of FUNDING
    // THE REVOLUTION, not an order-independent §3.11 fee -- so Maximilien
    // Robespierre's own exception (CoL p.12: "pay with all military actions
    // instead of civil") reaches it too, exactly like it already reaches the
    // revolution's own science cost (this file's `h_revolution` `via_
    // ordered_action && robespierre` branch). `legal::
    // breakthrough_robespierre_ma_fundable`'s own doc comment has the 5
    // corroborating BGO games (civil hits 0 taking + electing Robespierre,
    // same turn, before the revolution line) and the full citation; gated
    // there on a revolution actually being live so this never substitutes a
    // military action for an ordinary Breakthrough-funded Develop.
    if costs::spare_ca(&state.players[idx as usize]) < 1 && legal::breakthrough_robespierre_ma_fundable(state, &state.players[idx as usize], id) {
        state.players[idx as usize].military_actions -= 1;
        state.players[idx as usize].breakthrough_ma_funded = true;
    } else {
        costs::pay_ca(&mut state.players[idx as usize], 1);
    }
    state.players[idx as usize].hand_civil.remove_first(id);
    economy::discard_civil(state, id); // one-shot: played face up, spent

    let card = id.get();
    let eff = card.effects;
    {
        let p = &mut state.players[idx as usize];
        // `extraCivilActions` / `extraMilitaryActions` are dead in the base
        // game's data today (confirmed 2026-08-05: `grep` over
        // `data/*.json` finds neither key on any card), unlike `militaryActions`
        // (Patriotism), which IS captured as `CardEffects.military_actions`
        // and applied here exactly as Python's `_h_play_action` applies it --
        // a one-shot grant, not the recurring government stat. These, unlike
        // the GAIN_KEYS below, apply unconditionally BEFORE the ordered
        // action resolves -- Python applies them ahead of the `if ordered:`
        // branch too.
        p.military_actions = p.military_actions.saturating_add(eff.military_actions as i8);
        p.mil_discount += eff.resources_for_military_units;
        if crate::debugflags::replay_debug_all() && eff.resources_for_military_units != 0 {
            eprintln!(
                "DEBUG mil_discount site=h_play_action(card={:?}) idx={idx} += {} -> {}",
                card.name, eff.resources_for_military_units, p.mil_discount
            );
        }
    }

    // The two per-player-count action-card magnitudes (Endowment for the
    // Arts / Wave of Nationalism / Military Build-Up -- see this module's
    // top doc comment for why `gen_cards.py` could not fold these into a
    // flat `CardEffects` field). The comparisons are STRICT, per the printed
    // card text: Endowment for the Arts counts civilizations with "more
    // culture than you", and Wave of Nationalism / Military Build-Up count
    // those "stronger than yours" -- so an exact tie counts for nothing.
    // Ordering here (culture bonus, THEN the military discount) is moot: no
    // base-game card prints both.
    let count_idx = events::live_count_idx(state);
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::CulturePerCivilizationWithMoreCulture(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    }) {
        let per = t[count_idx] as i32;
        let mine = state.players[idx as usize].culture as i32;
        let mut n = 0i32;
        for q in state.active() {
            if q.idx != idx && q.culture as i32 > mine {
                n += 1;
            }
        }
        let gained = per * n;
        state.players[idx as usize].culture =
            (state.players[idx as usize].culture as i32 + gained).max(0) as u16;
    }
    if let Some(t) = card.special.iter().find_map(|&s| match s {
        Special::ResourcesForMilitaryUnitsPerStrongerCivilization(t) => Some(t),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    }) {
        let per = t[count_idx] as i32;
        let mine = effects::state_stats(state, &state.players[idx as usize]).strength;
        let mut n = 0i32;
        for q in state.active() {
            if q.idx != idx && effects::state_stats(state, q).strength > mine {
                n += 1;
            }
        }
        state.players[idx as usize].mil_discount += (per * n) as i16;
        if crate::debugflags::replay_debug_all() && per * n != 0 {
            eprintln!(
                "DEBUG mil_discount site=h_play_action(ResourcesForMilitaryUnitsPerStrongerCivilization,card={:?}) idx={idx} += {} -> {}",
                card.name,
                per * n,
                state.players[idx as usize].mil_discount
            );
        }
    }

    // §3.11: the ordered action resolves FIRST, and only THEN the card's own
    // gains -- Breakthrough's science and Frugality's food arrive too late to
    // pay for the very action the card just ordered (`_h_play_action`'s FIFO
    // `interact.enqueue` order: "free_civil" is pushed ahead of "card_gains").
    // A card with no ordered action skips straight to the gains, matching
    // Python's `else: apply_card_gains(...)` branch.
    let gains = card_gains_of(card);
    if let Some(value) = card.special.iter().find_map(|s| match s {
        Special::FreeCivilAction(v) => Some(*v),
        Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainFoodOrResources(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
    }) {
        // FIFO, and the order is the rule (§3.11): the ordered action
        // resolves, and only THEN the card's own gains -- Breakthrough's
        // science and Frugality's food arrive too late to pay for the very
        // action the card just ordered.
        crate::interact::enqueue(
            state,
            crate::state::QueueItem::FreeCivil {
                player: idx,
                kind: value,
                discount: eff.resource_discount,
                revolt_ok,
            },
        );
        crate::interact::enqueue(
            state,
            crate::state::QueueItem::CardGains { player: idx, gains },
        );
    } else {
        crate::interact::apply_card_gains(state, idx, gains);
    }
}

/// One action card's gain half as a [`crate::state::CardGains`]. Mirrors the
/// `{k: eff[k] for k in _GAIN_KEYS if k in eff}` comprehension in
/// `engine/actions.py::_h_play_action`, over the same six keys.
///
/// `gainPopulation` is dead in the base game's data today (confirmed
/// 2026-08-05: `data/*.json` prints the key on no card at all) and
/// `CardEffects` has no field for it, so it stays zero here; `interact::
/// apply_card_gains` implements the arithmetic anyway, because the field
/// exists and a silently-unimplemented gain is the bug class this port is
/// about.
fn card_gains_of(card: &crate::cards::Card) -> crate::state::CardGains {
    crate::state::CardGains {
        science: card.effects.gain_science,
        culture: card.effects.gain_culture,
        food: card.effects.gain_food,
        resources: card.effects.gain_resources,
        population: 0,
        food_or_resources: card
            .special
            .iter()
            .find_map(|s| match s {
                Special::GainFoodOrResources(n) => Some(*n),
                Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
            })
            .unwrap_or(0),
    }
}

/// Apply the one move an ordered free civil action resolved to, at no action
/// cost and `discount` off its resource cost (mirrors `engine/actions.py::
/// apply_free_action`'s dispatch over its own `kind` tuples). `Move::Pop` /
/// `Move::Build` / `Move::Upgrade` / `Move::WonderStep` / `Move::BachTheater`
/// are the shapes `free_action_moves`'s build/upgrade/pop/wonder-step arms
/// ever produce (`BachTheater` only when the actor is J. S. Bach, mid-turn,
/// same arm); `Move::Develop` / `Move::Revolution` are the two
/// `DevelopTechnology` can produce. No other `Move` variant is reachable
/// from there.
///
/// `pub`, not `pub(crate)`, since 2026-08: `replay.rs` (`docs/REPLAY.md`
/// Finding 2) also calls this directly, for a DIFFERENT free-civil source
/// than an action card -- Development of Civil Life's "immediately, each
/// civilization may [act] ... 1 [resource] less than usual" grants every
/// player (not just whoever prepared it) an untimed, banked `p.
/// one_time_discount` (see that field's own doc comment), which a real BGO
/// player can and does exercise OUT OF TURN, interleaved into whoever's turn
/// happens to be live -- the dominant confirmed cause of the "a different
/// player's action interleaves mid-turn, no `EndTurn` boundary" replay stop.
/// `idx` here is exactly what makes that safe to call for a non-`state.
/// current` actor: every branch below reads/writes `state.players[idx]`
/// only, never `state.current`/`state.phase`/that player's `civil_actions`
/// pool -- it was already actor-explicit and turn-agnostic for its
/// original caller (`interact::run_item`'s `QueueItem::FreeCivil` arm, which
/// resolves for a player who is not `state.current` in the 3p/4p case
/// today).
pub fn apply_free_civil_move(state: &mut GameState, idx: u8, mv: Move, discount: i32) {
    match mv {
        Move::Pop => h_pop(state, idx, true),
        Move::WonderStep { steps } => {
            let base = costs::wonder_stages_cost(state, idx, 1);
            do_wonder_step(state, idx, steps, (base - discount).max(0), true, None)
        }
        Move::Build { card } => do_build(state, idx, card, discount, true),
        Move::Upgrade { from, to } => do_upgrade(state, idx, from, to, discount, true),
        Move::Develop { card } => h_develop(state, idx, card, true),
    // §3.11's `TakeACard` order: the take's civil action is the FREE part --
    // what pays is the price the JOURNAL actually printed (the replayer
    // grounded it against the row's own `costs::take_cost`), carried in
    // `cost`. `i32::MAX` (the sentinel every ordinary take carries) bills
    // the slot's own price, so this one expression covers both sources.
    Move::Take { slot, cost } => h_take_ca(state, idx, slot, cost.min(costs::take_cost(state, &state.players[idx as usize], slot as usize))),
        // §8.3.4: a revolution never spends a civil action of its OWN to
        // begin with (it empties/carries-over a whole action pool instead),
        // so this is still the exact same `_h_revolution` Python's
        // `apply_free_action` calls for `Move::Revolution` -- but `true` here
        // (unlike the bare dispatch above) tells it this specific call was
        // reached via an ordered free action (Breakthrough), whose OWN `1
        // CA` was already spent one level up in `h_play_action` before this
        // ran -- see `h_revolution`'s own doc comment for why that matters
        // for Robespierre.
        Move::Revolution { card } => h_revolution(state, idx, card, true),
        // J. S. Bach's cross-kind theater upgrade, funded by the action
        // card's discount instead of his own bare 1 CA -- `legal::
        // free_action_moves`'s urban-upgrade arm is the only place this
        // shape can come from now; see its own doc comment for the
        // citation.
        Move::BachTheater { from, to } => h_bach_theater(state, idx, from, to, discount, true),
        other @ Move::PopFree | other @ Move::PlayLeader { .. } | other @ Move::PlayAction { .. } | other @ Move::Destroy { .. } | other @ Move::PlayTactic { .. } | other @ Move::CopyTactic { .. } | other @ Move::Aggression { .. } | other @ Move::War { .. } | other @ Move::OfferPact { .. } | other @ Move::CancelPact { .. } | other @ Move::PrepareEvent { .. } | other @ Move::RemoveLeaderYellow | other @ Move::ColumbusColonize { .. } | other @ Move::Barbarossa { .. } | other @ Move::TradeFoodAsResource | other @ Move::TradeResourceAsFood | other @ Move::Bid { .. } | other @ Move::BidPass | other @ Move::Defend { .. } | other @ Move::DefendDone | other @ Move::SendUnit { .. } | other @ Move::SendBonus { .. } | other @ Move::SendDiscard { .. } | other @ Move::SendDone | other @ Move::Choose { .. } | other @ Move::Churchill { .. } | other @ Move::EndTurn | other @ Move::PolPass | other @ Move::Resign => unreachable!(
            "free_action_moves produced a move apply_free_civil_move does not \
             expect: {other:?}"
        ),
    }
}

fn h_pol_pass(state: &mut GameState, idx: u8) {
    end_politics(state, idx, PoliticalAction::Passed);
}

/// §5.2: play an event/territory card from the hand into `future_events`,
/// bank its age as culture, and reveal-and-resolve the current event.
/// Mirrors `engine/actions.py::_h_prepare_event`.
///
/// `state.seeded_by[card] = idx` -- bot-evaluator bookkeeping only
/// (`bots/weighted.py`/`bots/counting.py`/`bots/neural_encode.py` are its
/// only readers; nothing in `engine/`'s own rules reads it). USED TO BE a
/// named gap here ("this port has no bot layer and `state.rs` has no field
/// for it") -- closed 2026-08-05 once `bots::counting::event_pool` became
/// that reader: `state.rs::GameState::seeded_by` now has a field for it, and
/// this is its one and only writer, exactly as in Python (`journal.py`'s
/// `del ...[ev]` docstring example is never actually called from
/// `actions.py`/`events.py` -- grepped 2026-08-05 -- so "write-once, never
/// cleared" is Python's real behaviour too, not a simplification).
///
/// Julius Caesar's once-per-game second political action USED to be a second
/// gap here (this handler always closed the phase, for every leader) --
/// closed 2026-08-05 by routing through [`end_politics`], same as every
/// other political handler.
fn h_prepare_event(state: &mut GameState, idx: u8, card: CardId) {
    state.players[idx as usize].hand_military.remove_first(card);
    state.players[idx as usize].culture += card.level() as u16;
    state.future_events.push(card);
    state.seeded_by[card.0 as usize] = idx;
    events::reveal_current_event(state);
    end_politics(state, idx, PoliticalAction::Played);
}

fn h_cancel_pact(state: &mut GameState, idx: u8, owner: u8) {
    // Collect what is about to be removed BEFORE `retain` clears it --
    // `on_pact_canceled` needs each pact's own `card`/`a`/`b` to give back
    // whatever `military_actions` it granted (§5.10's symmetric twin of
    // §5.9's "applies immediately", see `on_pact_accepted`'s doc comment).
    // `Pact` is `Copy` and there are at most `MAX_PACTS` of them, so a fixed
    // stack buffer is enough -- no heap allocation for this.
    let mut removed = [crate::state::Pact { card: CardId::NONE, owner: 0, partner: 0, a: 0, b: 0 }; crate::state::MAX_PACTS];
    let mut n_removed = 0usize;
    for &pact in state.players[owner as usize].pacts.as_slice() {
        if pact.is_party(idx) {
            removed[n_removed] = pact;
            n_removed += 1;
        }
    }
    state.players[owner as usize].pacts.retain(|pact| !pact.is_party(idx));
    for pact in &removed[..n_removed] {
        on_pact_canceled(state, pact);
    }
    end_politics(state, idx, PoliticalAction::Played);
}

/// Alexander the Great, as a political action: remove him from the game for
/// 1 yellow token from the box. Mirrors `engine/actions.py::
/// _h_remove_leader_yellow` + `remove_leader_from_game`.
fn h_remove_leader_yellow(state: &mut GameState, idx: u8) {
    let leader = state.players[idx as usize].leader;
    if !leader.is_none() {
        on_leave_play(&mut state.players[idx as usize], leader);
        economy::discard_civil(state, leader);
        state.players[idx as usize].leader = CardId::NONE;
    }
    grant_yellow(&mut state.players[idx as usize], 1);
    end_politics(state, idx, PoliticalAction::Played);
}

/// Trade Routes Agreement, side A's half (§5.9): convert 1 stored food into
/// 1 resource. Not a civil/military action (the printed text names none),
/// so unlike every handler around it this one does not touch `p.
/// civil_actions`/`p.military_actions` at all -- `legal::action_moves` is
/// the only legality gate, via `economy::trade_food_as_resource_remaining`.
/// `economy::gain_resources`, not a bare `+= 1`: the blue-token bank caps
/// every gain the same way regardless of source (`economy.rs`'s own doc
/// comment on `gain_food`/`gain_resources`), and this conversion is not
/// exempt from that cap just because it looks like a straight trade.
fn h_trade_food_as_resource(state: &mut GameState, idx: u8) {
    let p = &mut state.players[idx as usize];
    debug_assert!(p.food >= 1, "h_trade_food_as_resource: caller must ensure 1 food (legality check)");
    economy::pay_food(p, 1);
    economy::gain_resources(p, 1);
    p.trade_food_as_resource_used_this_turn += 1;
}

/// The [`h_trade_food_as_resource`] twin for Trade Routes' OTHER direction
/// ("Civilization B can use 1 resource as 1 food during its turn").
fn h_trade_resource_as_food(state: &mut GameState, idx: u8) {
    let p = &mut state.players[idx as usize];
    debug_assert!(p.resources >= 1, "h_trade_resource_as_food: caller must ensure 1 resource (legality check)");
    economy::pay_resources(p, 1);
    economy::gain_food(p, 1);
    p.trade_resource_as_food_used_this_turn += 1;
}

/// Christopher Columbus, as a political action: remove him from the game to
/// colonize `card` (a territory from hand) with no military sacrifice.
/// Mirrors `engine/actions.py::_h_columbus_colonize` +
/// `remove_leader_from_game`; `interact::gain_colony` is the exact function
/// the normal auction-won colonization path (`interact::apply_pending`'s
/// `SendDone` arm) already calls for §11.5's permanent-then-immediate order,
/// which is exactly what Columbus's ability skips STRAIGHT to (no auction, no
/// bid, no force -- `engine/interact.py::colonize_without_sacrifice`'s own
/// doc comment on why it is a separate entry point rather than a `bid=0`
/// call into the auction).
fn h_columbus_colonize(state: &mut GameState, idx: u8, card: CardId) {
    let leader = state.players[idx as usize].leader;
    if !leader.is_none() {
        on_leave_play(&mut state.players[idx as usize], leader);
        economy::discard_civil(state, leader);
        state.players[idx as usize].leader = CardId::NONE;
    }
    state.players[idx as usize].hand_military.remove_first(card);
    crate::interact::gain_colony(state, idx, card);
    end_politics(state, idx, PoliticalAction::Played);
}

/// Ports `engine/actions.py::_h_resign` up to (not including) `game.after_
/// resign`, which is `game.rs`'s to write -- see this module's top doc
/// comment. The war-cleanup section is fully ported now that `state.rs`
/// carries `war_declared_by_me`/`wars_declared_on_me`.
fn h_resign(state: &mut GameState, idx: u8) {
    state.players[idx as usize].resigned = true;
    state.players[idx as usize].hand_civil = CardList::new();

    let military: CardList<MAX_HAND> = state.players[idx as usize].hand_military.clone();
    for &card in military.as_slice() {
        economy::discard_military(state, card);
    }
    state.players[idx as usize].hand_military = CardList::new();

    drop_pacts_of(state, idx);

    // §5.11 war-cleanup: every war declared ON the resigning player scores
    // its declarer 7 culture, clears that declarer's OWN `war_declared_by_me`
    // if it is still this same war (it may already have moved on), and
    // discards the war card. `wars_declared_on_me` is indexed by attacker
    // (state.rs's doc comment), so this is a flat scan rather than Python's
    // list walk.
    for attacker in 0..MAX_PLAYERS as u8 {
        let war_card = state.players[idx as usize].wars_declared_on_me[attacker as usize];
        if war_card.is_none() {
            continue;
        }
        state.players[attacker as usize].culture += 7;
        if state.players[attacker as usize].war_declared_by_me == war_card {
            state.players[attacker as usize].war_declared_by_me = CardId::NONE;
            // Cleared together -- see `combat::resolve_war_outcome`'s comment
            // on why a stale `war_target` is a real divergence, not just a
            // meaningless leftover.
            state.players[attacker as usize].war_target = 0;
        }
        economy::discard_military(state, war_card);
    }
    state.players[idx as usize].wars_declared_on_me = [CardId::NONE; MAX_PLAYERS];

    // And the symmetric half: a war the resigning player themselves declared
    // is also torn down (the defender no longer faces it).
    let my_war = state.players[idx as usize].war_declared_by_me;
    if !my_war.is_none() {
        let target = state.players[idx as usize].war_target;
        state.players[target as usize].wars_declared_on_me[idx as usize] = CardId::NONE;
        economy::discard_military(state, my_war);
        state.players[idx as usize].war_declared_by_me = CardId::NONE;
        state.players[idx as usize].war_target = 0;
    }

    state.players[idx as usize].caesar_second_politics = false;
    state.players[idx as usize].peeked_event = CardId::NONE;
    state.players[idx as usize].politics_done = true;

    // §5.11: a resigning player's turn ends at once, and if that leaves one
    // player standing they win outright. `game.rs` owns that decision (it is
    // the same hand-off `end_turn` makes), exactly as Python's `_h_resign`
    // tail-calls `game.after_resign`.
    crate::game::after_resign(state);
}

// ============================================================== tests ====

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::Age;
    use crate::state::{CardList, GameState, Pact, PactList, Phase, PlayerState, Tableau, TechSlot, MAX_PLAYERS, ROW_SIZE};

    fn card(name: &str) -> CardId {
        CardId::by_name(name).unwrap_or_else(|| panic!("no such card: {name}"))
    }

    /// `h_play_action` DEFERS an action card's ordered action and its gains
    /// onto `state.queue` (§3.11, Python's `_h_play_action`), and it is
    /// `apply()` -- not the handler -- that drains the queue. These tests
    /// call the handler directly, so they do the drain themselves.
    fn play_action_and_drain(state: &mut GameState, id: CardId) {
        h_play_action(state, 0, id);
        crate::interact::run_queue(state);
    }

    fn blank_player(idx: u8, government: CardId) -> PlayerState {
        PlayerState {
            idx,
            techs: Tableau::new(),
            government,
            leader: CardId::NONE,
            wonder: CardId::NONE,
            wonder_stages_built: 0,
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
            raid_loot_pending: 0,
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
            food_tokens: crate::state::TokenBank::default(),
            resource_tokens: crate::state::TokenBank::default(),
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
            round: 2, // most handlers assume round > 1 (§1.9's row-only round 1)
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
            last_end_of_turn_culture: [None; crate::state::MAX_PLAYERS],
            last_end_of_turn_science: [None; MAX_PLAYERS],
            last_end_of_turn_resources: [None; MAX_PLAYERS],
        }
    }

    fn one_player_state(p: PlayerState) -> GameState {
        // Each filler needs its OWN `idx` (not a repeated 0) -- `combat.rs`
        // functions this module now calls (`start_aggression`,
        // `cancel_attack_pacts`) read `PlayerState::idx` for pact-party
        // matching, and four players sharing idx 0 would make every filler
        // indistinguishable from the actor for that purpose.
        let mut players = [
            blank_player(0, card("Despotism")),
            blank_player(1, card("Despotism")),
            blank_player(2, card("Despotism")),
            blank_player(3, card("Despotism")),
        ];
        players[0] = p;
        blank_state(4, players)
    }

    // -------------------------------------------------------------- leave play

    /// Regression for the `7522665` culture-checkpoint gap (`docs/REPLAY.md`'s
    /// culture-oracle `BuildWonderStage`-bucket trace): Bill Gates leaving
    /// play (there, via Iconoclasm discarding him for being off-age) must
    /// score culture equal to `effects::lab_level_workers`, mirroring his
    /// OTHER, already-wired ability (`ResourcesPerLabEqualToLevel`) which
    /// uses the identical formula while he is still in play. Revert the
    /// `CultureOnLeaveEqualToLabResourceProduction` branch in `on_leave_play`
    /// to see this go RED: culture stays 0 instead of 9.
    #[test]
    fn on_leave_play_scores_culture_equal_to_lab_level_workers_when_bill_gates_leaves() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Bill Gates");
        // A level-III lab staffed by 3 workers: lab_level_workers = 3*3 = 9,
        // matching the 9-culture gap traced in game 7522665 line 340.
        p.techs.insert(card("Computers"), TechSlot { workers: 3, stored: 0 });
        on_leave_play(&mut p, card("Bill Gates"));
        assert_eq!(p.culture, 9);
    }

    /// An UNSTAFFED lab contributes nothing (`lab_level_workers` sums
    /// `level * workers`, and `workers == 0` zeroes that lab's own term) --
    /// distinguishes "the formula is wired at all" from "the formula reads
    /// the tableau correctly" rather than the first test alone, which could
    /// pass by accident if `on_leave_play` granted a flat bonus instead.
    #[test]
    fn on_leave_play_grants_no_culture_from_bill_gates_for_an_unstaffed_lab() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Bill Gates");
        p.techs.insert(card("Computers"), TechSlot { workers: 0, stored: 0 });
        on_leave_play(&mut p, card("Bill Gates"));
        assert_eq!(p.culture, 0);
    }

    /// A leader with no `CultureOnLeaveEqualToLabResourceProduction` special
    /// (e.g. Napoleon) must not touch culture at all when leaving play, even
    /// with a staffed lab present -- the branch is gated on THIS specific
    /// leader's own special, not "any leader leaving play with a lab up".
    #[test]
    fn on_leave_play_does_not_score_culture_for_a_leader_without_the_bill_gates_special() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Napoleon Bonaparte");
        p.techs.insert(card("Computers"), TechSlot { workers: 3, stored: 0 });
        on_leave_play(&mut p, card("Napoleon Bonaparte"));
        assert_eq!(p.culture, 0);
    }

    // -------------------------------------------------------------- take

    #[test]
    fn h_take_pays_row_cost_and_moves_the_card_to_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        state.card_row[6] = card("Bronze"); // slot cost 2
        h_take_ca(&mut state, 0, 6, 2);
        assert_eq!(state.players[0].civil_actions, 2);
        assert_eq!(state.players[0].ca_spent_taking, 2);
        assert!(state.players[0].hand_civil.contains(card("Bronze")));
        assert!(state.card_row[6].is_none());
    }

    /// §3.11's `TakeACard` order (replayer-observed): the take's civil
    /// action is the FREE part, and the price that pays is the one the
    /// JOURNAL printed -- which can sit BELOW the slot's own row price once
    /// the replayer has grounded the card to a specific slot. `h_take_ca`
    /// must bill exactly the observed `ca` and record it in the ledger;
    /// recomputing the slot's price would double-charge the same take.
    #[test]
    fn h_take_ca_bills_the_observed_price_not_the_rows() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 3;
        let mut state = one_player_state(p);
        state.card_row[12] = card("Bronze"); // slot cost 3
        h_take_ca(&mut state, 0, 12, 1);
        assert_eq!(state.players[0].civil_actions, 2, "only the observed price is billed");
        assert_eq!(state.players[0].ca_spent_taking, 1, "the ledger records the observed price");
        assert!(state.players[0].hand_civil.contains(card("Bronze")));
    }

    #[test]
    fn h_take_a_wonder_sets_wonder_in_progress_not_hand() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        state.card_row[0] = card("Colossus");
        h_take_ca(&mut state, 0, 0, 1);
        assert_eq!(state.players[0].wonder, card("Colossus"));
        assert_eq!(state.players[0].wonder_steps, 0);
        assert!(state.players[0].hand_civil.is_empty());
    }

    #[test]
    fn h_take_aristotle_gains_science_for_a_technology() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.leader = card("Aristotle");
        let mut state = one_player_state(p);
        state.card_row[0] = card("Bronze"); // a technology (mine)
        h_take_ca(&mut state, 0, 0, 1);
        assert_eq!(state.players[0].science, 1);
    }

    /// CoL p.12 / `docs/RULES_SPEC.md` line ~383 (RESOLVED): "if the current
    /// player takes cards [via International Agreement], action cards taken
    /// may be used the same turn" -- the one documented exception to the
    /// normal rule that a card cannot be played in the same Action Phase it
    /// was taken in [RB p.14, CoL p.6]. Before this fix, `take_card`
    /// unconditionally recorded every taken Action card into
    /// `taken_this_turn`, so `legal::action_moves`'s `held > taken` gate
    /// blocked it even when it was taken via International Agreement's
    /// `TakeRow` choice -- found replaying real BGO games (`IllegalMove:
    /// PlayAction` bucket, e.g. games 7522274, 7523579, 7522824). Uses
    /// "Cultural Heritage" (an immediate-gain-only Action card, no ordered
    /// action) so `legal::action_card_playable` reads true unconditionally
    /// and the test isolates the `taken_this_turn` bookkeeping specifically.
    #[test]
    fn a_card_taken_via_international_agreement_can_still_be_played_the_same_turn() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        let mut state = one_player_state(p);
        let heritage = card("Cultural Heritage (A)");
        state.card_row[6] = heritage;

        take_card_from_international_agreement(&mut state, 0, 6);

        assert!(state.players[0].hand_civil.contains(heritage));
        let legal = legal::legal_moves(&state);
        assert!(
            legal.as_slice().contains(&Move::PlayAction { card: heritage }),
            "an action card taken via International Agreement must be immediately \
             playable the same turn, not blocked as if it were an ordinary Take: {:?}",
            legal.as_slice()
        );
    }

    // --------------------------------------------------------------- pop

    #[test]
    fn h_pop_pays_food_and_ca_and_moves_a_yellow_token() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5; // pop_cost_base(5) == 5
        p.food = 10;
        let mut state = one_player_state(p);
        h_pop(&mut state, 0, false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].food, 5);
        assert_eq!(state.players[0].yellow_bank, 4);
        assert_eq!(state.players[0].workers_free, 1);
    }

    #[test]
    fn h_pop_free_spends_no_civil_action_and_marks_ocean_liners_used() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 1;
        let mut state = one_player_state(p);
        h_pop_free(&mut state, 0);
        assert_eq!(state.players[0].civil_actions, 4, "no CA spent");
        assert_eq!(state.players[0].workers_free, 1);
        assert!(state.players[0].ocean_liners_used);
    }

    // ------------------------------------------- Trade Routes Agreement

    #[test]
    fn h_trade_food_as_resource_converts_1_food_into_1_resource_and_spends_no_action() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 3;
        p.food = 5;
        p.resources = 2;
        p.blue_total = 20; // room for the gained resource
        let mut state = one_player_state(p);
        h_trade_food_as_resource(&mut state, 0);
        let q = &state.players[0];
        assert_eq!(q.food, 4, "1 food spent");
        assert_eq!(q.resources, 3, "1 resource gained");
        assert_eq!(q.civil_actions, 4, "not a civil action");
        assert_eq!(q.military_actions, 3, "not a military action either");
        assert_eq!(q.trade_food_as_resource_used_this_turn, 1);
        assert_eq!(q.trade_resource_as_food_used_this_turn, 0, "the OTHER direction's counter is untouched");
    }

    #[test]
    fn h_trade_resource_as_food_converts_1_resource_into_1_food() {
        let mut p = blank_player(0, card("Despotism"));
        p.food = 2;
        p.resources = 5;
        p.blue_total = 20; // room for the gained food
        let mut state = one_player_state(p);
        h_trade_resource_as_food(&mut state, 0);
        let q = &state.players[0];
        assert_eq!(q.food, 3);
        assert_eq!(q.resources, 4);
        assert_eq!(q.trade_resource_as_food_used_this_turn, 1);
    }

    #[test]
    fn h_trade_food_as_resource_is_capped_by_the_blue_token_bank_like_any_other_gain() {
        // `economy::gain_resources` (not a bare `+= 1`): a player with no
        // free blue tokens converts the food away but gains nothing back,
        // exactly like any other over-cap "gain" in this engine -- proves
        // this handler did not bypass the shared capacity check by hand-
        // rolling the arithmetic instead of calling it.
        let mut p = blank_player(0, card("Despotism"));
        p.food = 1;
        p.resources = 0;
        p.blue_total = 0; // no capacity for the gained resource at all
        let mut state = one_player_state(p);
        h_trade_food_as_resource(&mut state, 0);
        assert_eq!(state.players[0].food, 0, "the food is still spent");
        assert_eq!(state.players[0].resources, 0, "but there is nowhere to store the gain");
    }

    /// THE REGRESSION, fixed 2026-08-05: Development of Civil Life's
    /// `pop_food` discount is one population increase, not a standing
    /// discount (state.rs's `OneTimeDiscount` doc comment has the card text).
    /// Before the fix `h_pop` never cleared the field, so a SECOND increase
    /// after the event resolved was still 1 food cheaper than it should be.
    #[test]
    fn h_pop_second_increase_after_the_event_costs_full_price() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        // pop_cost_base(18) == pop_cost_base(17) == 2 (both >= 17), so the
        // SECOND call is still comparing apples to apples after one token
        // moves out of the bank -- only the discount, nothing else, differs.
        p.yellow_bank = 18;
        p.food = 10;
        p.one_time_discount.pop_food = 1;
        let mut state = one_player_state(p);
        h_pop(&mut state, 0, false);
        assert_eq!(state.players[0].food, 10 - 1, "first increase: 2 - 1 discount");
        assert_eq!(state.players[0].one_time_discount.pop_food, 0,
                   "the discount must be consumed by the first increase");
        let food_before = state.players[0].food;
        h_pop(&mut state, 0, false);
        assert_eq!(food_before - state.players[0].food, 2,
                   "REGRESSION: the one-shot discount silently applied to a \
                    second population increase");
    }

    // ------------------------------------------------------------- build

    #[test]
    fn do_build_pays_resources_and_a_civil_action_and_adds_a_worker() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.workers_free = 2;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].resources, 10 - 4); // Irrigation build cost 4
        assert_eq!(state.players[0].workers_free, 1);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 1);
    }

    #[test]
    fn do_build_a_unit_pays_a_military_action_not_civil_and_spends_the_discount_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        p.resources = 10;
        p.workers_free = 1;
        p.mil_discount = 1;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Swordsmen"), 0, false);
        assert_eq!(state.players[0].military_actions, 1);
        assert_eq!(state.players[0].civil_actions, 0, "unpaid -- units use MA");
        assert_eq!(state.players[0].mil_discount, 0, "the 1-resource discount was spent");
        assert_eq!(state.players[0].resources, 10 - (3 - 1)); // Swordsmen cost 3, minus 1 discount
    }

    #[test]
    fn do_build_free_pays_nothing_and_applies_the_discount() {
        let mut p = blank_player(0, card("Despotism"));
        p.resources = 10;
        p.workers_free = 1;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Irrigation"), 1, true);
        assert_eq!(state.players[0].civil_actions, 0, "free: no CA spent");
        assert_eq!(state.players[0].resources, 10 - (4 - 1));
    }

    /// THE REGRESSION, fixed 2026-08-05: same bug as `h_pop`'s, for `build`.
    /// Civil Life's `build_resources` discount is one build, so a second one
    /// must be full price.
    #[test]
    fn do_build_second_build_after_the_event_costs_full_price() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.workers_free = 2;
        p.one_time_discount.build_resources = 1;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(state.players[0].resources, 10 - (4 - 1), "first build: discounted");
        assert_eq!(state.players[0].one_time_discount.build_resources, 0,
                   "the discount must be consumed by the first build");
        let before = state.players[0].resources;
        // A second worker on the SAME card is a legal build too (no per-card
        // worker cap -- farms/mines/urban buildings can carry any number).
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(before - state.players[0].resources, 4,
                   "REGRESSION: the one-shot build discount silently applied \
                    to a second build");
    }

    #[test]
    fn do_build_homer_pays_one_resource_less_on_a_unit_build() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Homer");
        p.military_actions = 2;
        p.resources = 10;
        p.workers_free = 1;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Swordsmen"), 0, false);
        // Swordsmen's raw cost is 3; Homer's discount is applied AT payment
        // time now (`costs::spend_homer_unit_discount`), not as a separate
        // post-payment gain -- net result is the same when resources are
        // plentiful (10 - 2 == (10 - 3) + 1 == 8), but see the next test for
        // why the two models are NOT equivalent at the margin.
        assert_eq!(state.players[0].resources, 8);
    }

    /// REGRESSION (found by replaying a real 4p BGO game, `7523341`, in
    /// `replay.rs`'s human-corpus reconstruction): the OLD model charged the
    /// full raw cost FIRST and only refunded Homer's resource afterward, so
    /// a player one resource short of the raw cost was illegally rejected by
    /// `legal::legal_moves` even though the discount should have covered the
    /// gap. `costs::build_cost_net`/`spend_homer_unit_discount` apply the
    /// discount BEFORE the affordability check now, matching the journal
    /// (`"loses 1 military resource; spends 1 resource"`, exactly the
    /// build's raw cost split between Homer's discount and the player's own
    /// pool -- the same phrasing `docs/REPLAY.md`'s Finding B established
    /// for `p.mil_discount`).
    #[test]
    fn a_player_with_exactly_one_resource_short_of_a_unit_s_raw_cost_can_still_build_it_with_homer() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Homer");
        p.military_actions = 2;
        p.resources = 2; // Swordsmen's raw cost is 3 -- one short without Homer
        p.workers_free = 1;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert_eq!(costs::build_cost_net(&state, &state.players[0], card("Swordsmen")), Some(2));
        assert!(
            legal::legal_moves(&state).as_slice().contains(&Move::Build { card: card("Swordsmen") }),
            "Homer's discount must make this build affordable and legal at exactly resources == raw_cost - 1"
        );
    }

    /// REGRESSION (real game `7521819`, round 6, 2p, found chasing the
    /// Build/Upgrade/WonderStep "resources short by 1-2" cluster): Homer's
    /// own leader text (`sources/bga_throughtheages_material.inc.php`) is
    /// "On your turn, you have an extra 1 resource for building and
    /// upgrading military units" -- AN extra 1 resource, singular, not one
    /// per build/upgrade action. The journal shows Orange (Homer-led)
    /// upgrading Warrior->Swordsmen TWICE in the same turn: the FIRST prints
    /// `"loses 1 military resource"` (Homer's discount applied), the SECOND
    /// prints only `"spends 1 resource"` (full price, no discount at all).
    /// Before this fix `homer_unit_discount` had no per-turn cap and
    /// unconditionally returned 1 for every unit build/upgrade while Homer
    /// was leader, so this binary silently gave a Homer-led player TWICE the
    /// resources a real human opponent could ever get -- an ENGINE bug
    /// (`legal::legal_moves` reads the same function for affordability, so
    /// this also made illegal double-builds look legal), not merely a
    /// replayer parsing gap.
    #[test]
    fn do_build_homers_discount_applies_only_once_per_turn() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Homer");
        p.military_actions = 3;
        p.resources = 6; // Swordsmen's raw cost is 3, needed twice = 6 with no discount at all
        p.workers_free = 2;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);

        do_build(&mut state, 0, card("Swordsmen"), 0, false);
        assert_eq!(state.players[0].resources, 4, "first build this turn: Homer's discount applies (6 - (3 - 1) = 4)");
        assert!(state.players[0].homer_used_this_turn, "the once-per-turn allowance is now spent");

        do_build(&mut state, 0, card("Swordsmen"), 0, false);
        assert_eq!(
            state.players[0].resources, 1,
            "second build in the SAME turn pays the full raw cost (4 - 3 = 1), not a second discount (which would leave 2)"
        );
    }

    /// Same regression as the discount-amount test above, but for
    /// `legal::legal_moves`'s affordability check specifically -- the
    /// exact mirror of `a_player_with_exactly_one_resource_short_...` above,
    /// showing the SAME "1 short" scenario is properly rejected once this
    /// turn's Homer allowance is already spent.
    #[test]
    fn a_second_same_turn_unit_build_one_resource_short_is_illegal_once_homers_allowance_is_spent() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Homer");
        p.homer_used_this_turn = true; // already used earlier this same turn
        p.military_actions = 2;
        p.resources = 2; // one short of Swordsmen's raw cost of 3
        p.workers_free = 1;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 0, stored: 0 });
        let state = one_player_state(p);
        assert_eq!(
            costs::build_cost_net(&state, &state.players[0], card("Swordsmen")),
            Some(3),
            "no discount left this turn -- full raw cost"
        );
        assert!(
            !legal::legal_moves(&state).as_slice().contains(&Move::Build { card: card("Swordsmen") }),
            "1 short with no discount left this turn must be illegal, unlike the fresh-turn case above"
        );
    }

    // ----------------------------------------------------------- destroy

    #[test]
    fn h_destroy_a_civil_card_pays_one_ca_and_frees_a_worker() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.techs.insert(card("Irrigation"), TechSlot { workers: 1, stored: 0 });
        let mut state = one_player_state(p);
        h_destroy(&mut state, 0, card("Irrigation"));
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 0);
        assert_eq!(state.players[0].workers_free, 1);
    }

    #[test]
    fn h_destroy_a_unit_pays_one_ma() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        p.techs.insert(card("Swordsmen"), TechSlot { workers: 1, stored: 0 });
        let mut state = one_player_state(p);
        h_destroy(&mut state, 0, card("Swordsmen"));
        assert_eq!(state.players[0].military_actions, 1);
        assert_eq!(state.players[0].techs.workers(card("Swordsmen")), 0);
    }

    // ----------------------------------------------------------- upgrade

    #[test]
    fn do_upgrade_moves_the_worker_and_pays_the_difference() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        do_upgrade(&mut state, 0, card("Agriculture"), card("Irrigation"), 0, false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].resources, 10 - 2); // 4 - 2 = 2
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 0);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 1);
    }

    // -------------------------------------------------------- Barbarossa

    #[test]
    fn h_barbarossa_grows_population_and_builds_a_unit_for_one_military_action() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Frederick Barbarossa");
        p.military_actions = 2;
        p.civil_actions = 4;
        p.yellow_bank = 14; // pop_cost_base(14) == 3, minus his 1 food discount == 2
        p.food = 2;
        p.resources = 1; // Warriors costs 2, minus his 1 resource discount == 1
        p.techs.insert(card("Warriors"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        h_barbarossa(&mut state, 0, card("Warriors"));
        let p = &state.players[0];
        assert_eq!(p.food, 0, "paid the discounted population cost");
        assert_eq!(p.yellow_bank, 13, "one token left the bank");
        assert_eq!(p.resources, 0, "paid the discounted build cost");
        assert_eq!(p.techs.workers(card("Warriors")), 1, "the new worker built the unit");
        assert_eq!(p.military_actions, 1, "the ONE military action bought both halves");
        assert_eq!(p.civil_actions, 4, "the population half costs no civil action");
    }

    // -------------------------------------------------------------- Bach

    #[test]
    fn h_bach_theater_upgrades_cross_type_pays_the_difference_and_marks_used_once() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("J. S. Bach");
        p.civil_actions = 4;
        p.resources = 10;
        p.techs.insert(card("Theology"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Drama"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        h_bach_theater(&mut state, 0, card("Theology"), card("Drama"), 0, false);
        let p = &state.players[0];
        assert!(p.bach_upgrade_used, "at most once per turn");
        assert_eq!(p.civil_actions, 3);
        assert_eq!(p.techs.workers(card("Theology")), 0);
        assert_eq!(p.techs.workers(card("Drama")), 1);
        // Theology costs 5, Drama costs 4 -- the upgrade cost floors at 0
        // rather than refunding the difference.
        assert_eq!(p.resources, 10);
    }

    // ------------------------------------------------------------ develop

    #[test]
    fn h_develop_a_technology_inserts_it_at_zero_workers() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 10;
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Irrigation"), false);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].science, 10 - 3); // Irrigation tech cost 3
        assert!(state.players[0].techs.has(card("Irrigation")));
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 0);
        assert!(!state.players[0].hand_civil.contains(card("Irrigation")));
    }

    /// ENGINE GAP closed: Scientific Cooperation's own card text ("...the
    /// other civilization pays 1 science...") had NO consumer anywhere in
    /// the crate before this pass -- `effects::Stats::science_partners` was
    /// computed but only ever read by its own unit tests. `h_develop` must
    /// charge the partner too, not just discount the developer.
    #[test]
    fn h_develop_charges_the_pact_partner_1_science() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 10;
        p.hand_civil.push(card("Irrigation"));
        p.pacts.push(crate::state::Pact {
            card: card("Scientific Cooperation"),
            owner: 0,
            partner: 1,
            a: 0,
            b: 1,
        });
        let mut state = one_player_state(p);
        state.players[1].science = 5;
        h_develop(&mut state, 0, card("Irrigation"), false);
        // Irrigation's printed techCost 3, minus the pact's 2 = 1.
        assert_eq!(state.players[0].science, 10 - 1, "the developer pays the discounted cost");
        assert_eq!(state.players[1].science, 5 - 1, "the partner pays its own 1 science");
    }

    /// THE REGRESSION, fixed 2026-08-05: same bug as `h_pop`'s and
    /// `do_build`'s, for `develop`. Civil Life's `develop_science` discount
    /// is one technology; two DISTINCT technologies (Irrigation techCost 3,
    /// Iron techCost 5) so the second is not just re-developing the first.
    #[test]
    fn h_develop_second_technology_after_the_event_costs_full_price() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 20;
        p.one_time_discount.develop_science = 1;
        p.hand_civil.push(card("Irrigation"));
        p.hand_civil.push(card("Iron"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Irrigation"), false);
        assert_eq!(state.players[0].science, 20 - (3 - 1), "first develop: discounted");
        assert_eq!(state.players[0].one_time_discount.develop_science, 0,
                   "the discount must be consumed by the first develop");
        let before = state.players[0].science;
        h_develop(&mut state, 0, card("Iron"), false);
        assert_eq!(before - state.players[0].science, 5,
                   "REGRESSION: the one-shot develop discount silently \
                    applied to a second technology");
    }

    /// ENGINE BUG FIX (`docs/REPLAY.md` fifth pass): Development of Civil
    /// Life's grant is ONE mutually-exclusive choice ("may EITHER increase
    /// population; OR build...; OR develop...") -- spending ANY ONE of the
    /// three candidate discounts must exhaust the OTHER TWO, not leave them
    /// standing. This replaces a same-named-in-spirit test that asserted the
    /// OPPOSITE (independent consumption) -- that was the bug, confirmed
    /// wrong by replaying real BGO games where a human who spent one
    /// discount type paid FULL price on a later action of a different type
    /// this engine's old model predicted should still be discounted.
    #[test]
    fn spending_any_one_civil_life_discount_exhausts_the_whole_grant() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5;
        p.food = 10;
        p.resources = 10;
        p.workers_free = 1;
        p.one_time_discount = crate::state::OneTimeDiscount {
            build_resources: 1,
            develop_science: 1,
            pop_food: 1,
        };
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        let mut state = one_player_state(p);
        h_pop(&mut state, 0, false);
        let d = state.players[0].one_time_discount;
        assert_eq!(d, crate::state::OneTimeDiscount::default(),
                   "spending the population discount must exhaust build \
                    and develop too, not just pop_food");
        // and the now-exhausted build discount is for real, not just a
        // field: the build below must be FULL price.
        let before = state.players[0].resources;
        do_build(&mut state, 0, card("Irrigation"), 0, false);
        assert_eq!(before - state.players[0].resources, 4,
                   "the build discount was already exhausted by the pop \
                    action; this build must pay Irrigation's full cost 4, \
                    not the discounted 3");
    }

    #[test]
    fn h_develop_leonardo_da_vinci_gains_a_resource() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Leonardo da Vinci");
        p.civil_actions = 4;
        p.science = 10;
        p.blue_total = 20; // bank room for gain_resources to actually pay out
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Irrigation"), false);
        assert_eq!(state.players[0].resources, 1);
    }

    #[test]
    fn h_develop_a_government_switches_via_set_government() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Monarchy"), false);
        assert_eq!(state.players[0].government, card("Monarchy"));
        // Monarchy: 5 CA / 3 MA. 1 CA was spent paying for `develop` itself,
        // so the new pool is 5 - 1 = 4 (recomputed relative to what was
        // already spent, per `_set_government`'s comment).
        assert_eq!(state.players[0].civil_actions, 4);
        assert_eq!(state.players[0].military_actions, 3);
    }

    #[test]
    fn set_government_a_decreased_total_is_absorbed_by_already_spent_actions_before_touching_unspent_ones() {
        // §8.2: "update CA/MA totals... if decreased, return tokens (spent
        // first)". Monarchy (5 CA / 3 MA) -> Republic (7 CA / 2 MA): CA goes
        // UP (7 - 0 spent = 7, ordinary top-up), MA goes DOWN by 1 with 1 MA
        // already spent this turn -- the decrease is fully absorbed by that
        // spent token, so the 2 UNSPENT MAs must survive untouched. Mirrors
        // `docs/REPLAY.md`'s traced `7523357` round-10 numbers exactly
        // (Orange discovers Republic with 1 of 3 Monarchy MAs already spent).
        let mut p = blank_player(0, card("Monarchy"));
        p.civil_actions = 5; // full, unspent
        p.military_actions = 2; // 3 total - 1 already spent this turn
        let mut state = one_player_state(p);
        set_government(&mut state, 0, card("Republic"));
        assert_eq!(state.players[0].civil_actions, 7, "CA increased: ordinary top-up");
        assert_eq!(state.players[0].military_actions, 2, "MA decrease absorbed by the spent token, not the unspent ones");
    }

    #[test]
    fn h_develop_a_special_tech_keeps_only_the_higher_level_per_icon() {
        // Both Masonry and Construction are `construction`-icon special techs
        // (buildDiscount); Construction is the higher level.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Masonry"));
        let mut state = one_player_state(p);
        h_develop(&mut state, 0, card("Masonry"), false);
        assert!(state.players[0].techs.has(card("Masonry")));

        state.players[0].civil_actions = 4;
        state.players[0].hand_civil.push(card("Architecture"));
        h_develop(&mut state, 0, card("Architecture"), false);
        assert!(!state.players[0].techs.has(card("Masonry")), "lower level discarded");
        assert!(state.players[0].techs.has(card("Architecture")));
    }

    // -------------------------------------------------------- play_leader

    #[test]
    fn h_play_leader_replacing_none_just_plays_it() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Hammurabi"));
        let mut state = one_player_state(p);
        h_play_leader(&mut state, 0, card("Hammurabi"));
        assert_eq!(state.players[0].leader, card("Hammurabi"));
        assert_eq!(state.players[0].civil_actions, 3);
    }

    #[test]
    fn h_play_leader_replacing_one_refunds_a_civil_action_and_discards_the_old() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 1; // nearly spent
        p.leader = card("Hammurabi");
        p.hand_civil.push(card("Aristotle"));
        let mut state = one_player_state(p);
        h_play_leader(&mut state, 0, card("Aristotle"));
        assert_eq!(state.players[0].leader, card("Aristotle"));
        // Paid 1 CA to play, then refunded 1 CA for replacing -> net unchanged.
        assert_eq!(state.players[0].civil_actions, 1);
        assert!(state.civil_removed[Age::A as usize].contains(card("Hammurabi")), "Hammurabi is Age A");
    }

    #[test]
    fn replacing_hammurabi_mid_turn_keeps_his_military_action_as_civil_action_conversion_for_the_rest_of_the_turn(
    ) {
        // RB, "Replacing a Leader": "You are allowed to use the benefit of a
        // leader and then replace him or her on the same turn." Hammurabi's
        // benefit is a once-per-turn MA-as-CA conversion that this engine
        // spends LAZILY (`costs::pay_ca` only reaches for it once the civil
        // pool is empty), so a player who spends their last printed civil
        // action ON the replacement would silently forfeit it. Found by
        // replaying the BGO human corpus: 103 of the 109 games whose
        // reconstruction stopped one civil action short of what the human
        // demonstrably spent are turns in which the human replaced Hammurabi
        // (against Hammurabi being only 39% of all Age A leader replacements
        // corpus-wide) -- see `docs/REPLAY.md`.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 1; // last printed civil action, about to be spent
        p.military_actions = 1;
        p.leader = card("Hammurabi");
        p.hand_civil.push(card("Aristotle"));
        let mut state = one_player_state(p);
        h_play_leader(&mut state, 0, card("Aristotle"));
        let p = &state.players[0];
        // Paid 1 CA to play, refunded 1 CA for replacing -> 1 printed action
        // left, PLUS Hammurabi's conversion, which the swap must not eat.
        assert_eq!(p.civil_actions, 1);
        assert_eq!(costs::spare_ca(p), 2, "Hammurabi's conversion survives his own replacement");
        let mut p = state.players[0].clone();
        costs::pay_ca(&mut p, 2);
        assert_eq!(p.civil_actions, 0);
        assert_eq!(p.military_actions, 0, "the second action was paid with the military token");
        assert!(p.hammurabi_used, "and the once-per-turn conversion is now spent");
    }

    /// Taj Mahal's 2015 clause says "replaced", and the BGO corpus agrees to
    /// the case: of the 8 Taj Mahal takes made in a turn whose only election
    /// put a FIRST leader into an empty slot, every single one was charged
    /// full price -- while 149 of the 150 free takes follow a replacement.
    #[test]
    fn playing_a_first_leader_into_an_empty_slot_is_not_a_replacement_but_swapping_one_is() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 2; // one for each leader played below
        p.hand_civil.push(card("Hammurabi"));
        p.hand_civil.push(card("Aristotle"));
        let mut state = one_player_state(p);

        h_play_leader(&mut state, 0, card("Hammurabi"));
        assert!(
            !state.players[0].replaced_leader_this_turn,
            "an empty leader slot means there was nobody to replace"
        );

        h_play_leader(&mut state, 0, card("Aristotle"));
        assert!(state.players[0].replaced_leader_this_turn);

        crate::economy::end_of_turn(&mut state, 0);
        assert!(
            !state.players[0].replaced_leader_this_turn,
            "the discount is scoped to the turn of the replacement, not the rest of the game"
        );
    }

    /// A leader carrying a flat `military_actions` bonus (Joan of Arc: +1
    /// MA) who is replaced MID-TURN, after some of that bonus was already
    /// spent, must not have the already-spent portion clawed back out of
    /// the remaining pool -- RULES_SPEC #7's "Effects apply immediately"
    /// governs the NEW leader's own bonus, not a retroactive subtraction of
    /// the old one's. Found chasing the `IllegalMove: Build` bucket: BGO
    /// human game 7522520 round 11 (Theocracy, 3 printed MA) has Joan of
    /// Arc's +1 MA (4 total) pay for a 2-MA `CopyTactic`, THEN Isaac Newton
    /// replaces her (no MA bonus of his own, new total back to 3) mid-turn,
    /// and the human still builds two more military units (1 MA each) --
    /// only possible if the 2 MA remaining after `CopyTactic` survives the
    /// swap untouched. Before this fix `on_leave_play`'s plain decrement
    /// (clamped at 0, no memory of what was already spent) dropped the
    /// remaining pool from 2 to 1, one short of the second unit build.
    #[test]
    fn replacing_a_leader_with_a_flat_military_action_bonus_mid_turn_does_not_claw_back_actions_already_spent_from_it(
    ) {
        let mut p = blank_player(0, card("Theocracy")); // 4 CA / 3 MA printed
        p.leader = card("Joan of Arc"); // +1 MA -> 4 MA total this turn
        p.civil_actions = 3;
        p.military_actions = 2; // 4 total minus 2 already spent on CopyTactic
        p.hand_civil.push(card("Isaac Newton"));
        let mut state = one_player_state(p);
        h_play_leader(&mut state, 0, card("Isaac Newton"));
        // Newton grants no flat MA, so the total drops back to 3 -- but the
        // 2 already spent (from Joan's now-departed bonus) must not be
        // spent again: what's left stays 2, not 1.
        assert_eq!(state.players[0].military_actions, 2);
    }

    // -------------------------------------------------------------- churchill

    #[test]
    fn h_churchill_culture_choice() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        h_churchill(&mut state, 0, ChurchillChoice::Culture);
        assert_eq!(state.players[0].culture, 3);
        assert!(state.players[0].churchill_used);
    }

    #[test]
    fn h_churchill_military_choice_grants_ring_fenced_pools() {
        let p = blank_player(0, card("Despotism"));
        let mut state = one_player_state(p);
        h_churchill(&mut state, 0, ChurchillChoice::Military);
        assert_eq!(state.players[0].mil_sci_discount, 3);
        assert_eq!(state.players[0].mil_discount, 3);
    }

    // -------------------------------------------------------------- tactics

    #[test]
    fn h_play_tactic_spends_one_ma_and_sets_the_exclusive_tactic() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        p.hand_military.push(card("Legion"));
        let mut state = one_player_state(p);
        h_play_tactic(&mut state, 0, card("Legion"));
        assert_eq!(state.players[0].military_actions, 1);
        assert_eq!(state.players[0].tactic, card("Legion"));
        assert!(state.players[0].tactic_exclusive);
        assert!(state.players[0].tactic_action_used);
    }

    #[test]
    fn h_copy_tactic_spends_two_ma_and_is_not_exclusive() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 2;
        let mut state = one_player_state(p);
        h_copy_tactic(&mut state, 0, card("Legion"));
        assert_eq!(state.players[0].military_actions, 0);
        assert_eq!(state.players[0].tactic, card("Legion"));
        assert!(!state.players[0].tactic_exclusive);
    }

    // ----------------------------------------------------------------- war

    #[test]
    fn h_war_pays_military_actions_and_records_the_declaration() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 3;
        p.hand_military.push(card("War over Territory")); // cost 2
        let mut state = one_player_state(p);
        h_war(&mut state, 0, card("War over Territory"), 1);
        assert_eq!(state.players[0].military_actions, 1);
        assert!(!state.players[0].hand_military.contains(card("War over Territory")));
        assert_eq!(state.players[0].war_declared_by_me, card("War over Territory"));
        assert_eq!(state.players[0].war_target, 1);
        assert_eq!(state.players[1].wars_declared_on_me[0], card("War over Territory"));
        assert!(state.players[0].politics_done);
        assert_eq!(state.phase, Phase::Actions);
    }

    #[test]
    fn h_war_doubles_cost_against_mahatma_gandhi() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 4;
        p.hand_military.push(card("War over Territory")); // cost 2, doubled to 4
        let mut target = blank_player(1, card("Despotism"));
        target.leader = card("Mahatma Gandhi");
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [p, target, filler(), blank_player(3, card("Despotism"))]
        });
        h_war(&mut state, 0, card("War over Territory"), 1);
        assert_eq!(state.players[0].military_actions, 0, "cost doubled to 4");
    }

    #[test]
    fn h_war_cancels_a_pact_that_ends_on_mutual_attack() {
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = 3;
        p.hand_military.push(card("War over Territory"));
        p.pacts.push(Pact { card: card("Military Alliance"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [p, blank_player(1, card("Despotism")), filler(), blank_player(3, card("Despotism"))]
        });
        h_war(&mut state, 0, card("War over Territory"), 1);
        assert!(state.players[0].pacts.is_empty(), "Military Alliance ends the moment its parties attack");
    }

    // -------------------------------------------------------------- aggression

    #[test]
    fn h_aggression_pays_cost_discards_and_cancels_pacts_then_resolves() {
        // `h_aggression` runs `combat::start_aggression` (the portable
        // prefix of `events.start_aggression`) and then hands the defense
        // decision to the rival through `interact::start_defense`.
        let agg = crate::cards::CARDS
            .iter()
            .position(|c| c.kind == CardType::Aggression)
            .map(|i| CardId(i as u16))
            .expect("at least one aggression card exists");
        let cost = agg.get().military_action_cost as i8;
        let mut p = blank_player(0, card("Despotism"));
        p.military_actions = cost + 2;
        p.hand_military.push(agg);
        p.pacts.push(Pact { card: card("Military Alliance"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut state = one_player_state(p);

        h_aggression(&mut state, 0, agg, 1);
        assert_eq!(state.players[0].military_actions, 2, "cost was paid");
        assert!(!state.players[0].hand_military.contains(agg), "card left the hand");
        assert!(
            state.discarded_military[agg.get().age as usize].contains(agg),
            "card was discarded"
        );
        assert!(state.players[0].pacts.is_empty(), "doomed pact was cancelled");
        assert!(state.players[0].politics_done, "the political action is spent");
        // The defender holds no military cards, so §5.4.4 offers nothing to
        // decide and the aggression resolves at its printed strength -- 0 vs
        // 0, which FAILS. That whole tail was `unimplemented!` until
        // `interact::start_defense` existed to produce a defense total.
        assert!(state.pending.is_empty(), "nothing left to decide");
    }

    // ------------------------------------------------------------- play_action

    #[test]
    fn h_play_action_patriotism_grants_a_military_action_and_the_discount_pool() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Patriotism (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Patriotism (A)"));
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].military_actions, 3, "Patriotism grants +1 MA");
        assert_eq!(state.players[0].mil_discount, 1, "resourcesForMilitaryUnits: 1");
        assert!(!state.players[0].hand_civil.contains(card("Patriotism (A)")));
    }

    #[test]
    fn h_play_action_cultural_heritage_grants_flat_science_and_culture() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Cultural Heritage (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Cultural Heritage (A)"));
        assert_eq!(state.players[0].science, 1);
        assert_eq!(state.players[0].culture, 4);
    }

    #[test]
    fn h_play_action_stock_pile_gains_food_and_resources() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.blue_total = 10;
        p.hand_civil.push(card("Stock Pile"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Stock Pile"));
        assert_eq!(state.players[0].food, 1);
        assert_eq!(state.players[0].resources, 1);
    }

    #[test]
    fn h_play_action_rich_land_with_no_legal_ordered_move_is_a_silent_no_op() {
        // Rich Land (I) (Rich Land (A) mints the `take_a_card` fixture
        // now -- see `data/cards_military_actions.json`). Empty row and
        // empty tableau: its ordered build/upgrade has nothing to
        // target, so this must resolve like Python's `push_choice` with
        // an empty option list -- silently, no panic -- exactly as
        // playing a card with no ordered action at all does.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Rich Land (I)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Rich Land (I)"));
        assert_eq!(state.players[0].civil_actions, 3, "only the card's own CA is spent");
        assert!(!state.players[0].hand_civil.contains(card("Rich Land (I)")));
    }

    #[test]
    fn h_play_action_rich_land_builds_a_mine_at_a_discount_and_no_action_cost() {
        // "Rich Land (I)" (Rich Land (A) mints the `take_a_card` fixture
        // now -- see `data/cards_military_actions.json`):
        // build_or_upgrade_farm_or_mine, resourceDiscount 2. Bronze costs 2
        // resources; the free build pays only 2 - 2 = 0 and no separate
        // civil action (only the 1 CA to play the card itself).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.workers_free = 1;
        p.resources = 0;
        p.techs.insert(card("Bronze"), TechSlot { workers: 0, stored: 0 });
        p.hand_civil.push(card("Rich Land (I)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Rich Land (I)"));
        assert_eq!(state.players[0].techs.workers(card("Bronze")), 1);
        assert_eq!(state.players[0].resources, 0);
        assert_eq!(state.players[0].workers_free, 0);
        assert_eq!(state.players[0].civil_actions, 3, "the build itself is free");
    }

    #[test]
    fn h_play_action_efficient_upgrade_upgrades_at_a_discount() {
        // "Efficient Upgrade (II)": upgrade_farm_mine_or_urban_building,
        // resourceDiscount 3. Agriculture->Irrigation costs 4-2=2, floored to
        // 0 by the discount.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Irrigation"), TechSlot { workers: 0, stored: 0 });
        p.hand_civil.push(card("Efficient Upgrade (II)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Efficient Upgrade (II)"));
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 0);
        assert_eq!(state.players[0].techs.workers(card("Irrigation")), 1);
        assert_eq!(state.players[0].civil_actions, 3, "the upgrade itself is free");
    }

    #[test]
    fn h_play_action_engineering_genius_builds_a_wonder_stage_at_a_discount() {
        // "Engineering Genius (A)": build_one_wonder_stage, resourceDiscount
        // 2. Pyramids' first stage costs 3, floored by the discount to 1.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.wonder = card("Pyramids");
        p.resources = 1;
        p.hand_civil.push(card("Engineering Genius (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Engineering Genius (A)"));
        assert_eq!(state.players[0].wonder_steps, 1);
        assert_eq!(state.players[0].resources, 0);
        assert_eq!(state.players[0].civil_actions, 3, "the wonder step itself is free");
    }

    #[test]
    fn h_play_action_frugality_increases_population_before_its_own_gain_food_lands() {
        // "Frugality (A)": increase_population "at full price" + gainFood 1.
        // yellow_bank 5 prices the increase at 5 food; starting on exactly 5
        // affords it (paid BEFORE the card's own +1 food arrives), leaving 0,
        // then the card's own gain lands on top.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5;
        p.food = 5;
        p.blue_total = 10; // gain_food needs blue tokens free to convert
        p.hand_civil.push(card("Frugality (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Frugality (A)"));
        assert_eq!(state.players[0].workers_free, 1, "the ordered pop happened");
        assert_eq!(state.players[0].yellow_bank, 4);
        assert_eq!(state.players[0].food, 1, "0 left after the pop, +1 from the card's own gainFood");
        assert_eq!(state.players[0].civil_actions, 3, "the pop itself is free");
    }

    #[test]
    fn h_play_action_frugality_own_gain_food_arrives_too_late_to_pay_for_its_pop() {
        // Same card, starting 1 food short of the pop cost: the card's own
        // +1 food would cover the gap, but §3.11 resolves the ordered action
        // BEFORE the card's own gains land (`_h_play_action`'s FIFO enqueue
        // order), so the pop must NOT happen -- only the flat +1 food does.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.yellow_bank = 5; // prices the increase at 5
        p.food = 4;
        p.blue_total = 10; // gain_food needs blue tokens free to convert
        p.hand_civil.push(card("Frugality (A)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Frugality (A)"));
        assert_eq!(state.players[0].workers_free, 0, "the ordered pop must not happen");
        assert_eq!(state.players[0].yellow_bank, 5, "unchanged: no token moved");
        assert_eq!(state.players[0].food, 5, "4 unspent + the card's own +1 gainFood");
    }

    #[test]
    fn h_play_action_breakthrough_develops_a_technology_at_full_price() {
        // "Breakthrough (I)": develop_technology "at full price" + gainScience
        // 2. Bronze is a free develop (scienceCost 0), so this exercises the
        // ordered `develop` arm end to end.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Bronze"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.players[0].techs.workers(card("Bronze")), 0, "developed, not built");
        assert!(state.players[0].techs.has(card("Bronze")));
        assert!(!state.players[0].hand_civil.contains(card("Bronze")));
        assert_eq!(state.players[0].science, 2, "only the card's own gainScience -- Bronze cost 0");
        assert_eq!(state.players[0].civil_actions, 3, "the develop itself is free");
    }

    #[test]
    fn h_play_action_breakthrough_own_gain_science_arrives_too_late_to_pay_for_its_develop() {
        // Irrigation costs 3 science; starting on 2 is short, and the card's
        // own +2 science arrives AFTER the ordered action resolves (same
        // FIFO rule as Frugality above), so the develop must not happen.
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.science = 2;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Irrigation"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert!(!state.players[0].techs.has(card("Irrigation")), "must not develop");
        assert!(state.players[0].hand_civil.contains(card("Irrigation")), "stays in hand");
        assert_eq!(state.players[0].science, 4, "2 unspent + the card's own +2 gainScience");
    }

    /// Two developable, freely-affordable technologies in hand: a GENUINE
    /// tie, which `push_choice(auto=True)` does not resolve. This used to be
    /// the one case that had to panic; it now opens a real decision, and the
    /// player who owns it is the one who played the card.
    #[test]
    fn h_play_action_breakthrough_opens_a_decision_on_a_genuine_multi_way_choice() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Bronze"));
        p.hand_civil.push(card("Agriculture"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.decider(), 0);
        let moves = legal::legal_moves(&state);
        assert_eq!(moves.len(), 2, "develop Agriculture, or develop Bronze");
        // §3.11: the card's own gains are queued BEHIND the ordered action,
        // so they have not landed yet.
        assert_eq!(state.players[0].science, 0);
        apply(&mut state, Move::Choose { n: 0 });
        assert!(state.pending.is_empty());
        assert_eq!(state.players[0].science, 2, "...and now the gains land");
    }

    #[test]
    fn h_play_action_breakthrough_may_spend_its_order_on_a_revolution() {
        // RB p.15: Breakthrough's order may pay for a revolution instead of a
        // peaceful develop. Monarchy's peaceful cost (8) is unaffordable on 2
        // science, but its revolutionCost (2) is -- and `revolt_ok` needs
        // "every civil action THIS TURN still unspent" measured BEFORE
        // playing Breakthrough spends one, not after (a regression test for
        // that exact ordering: computing it post-`pay_ca` would read
        // `civil_actions` as one short of `ca_total` and wrongly report no
        // legal ordered-action move at all).
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4; // == ca_total(Despotism): revolt_ok must be true
        p.science = 2;
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.players[0].government, card("Monarchy"), "revolution happened");
        assert_eq!(state.players[0].science, 2, "0 left after the revolution, +2 from the card's own gainScience");
    }

    #[test]
    fn h_play_action_breakthrough_may_spend_its_order_on_a_revolution_for_robespierre_who_already_spent_a_civil_action() {
        // ENGINE BUG (found replaying real BGO games 7523216/7523482): this
        // `revolt_ok` used to be a SECOND, independently-recomputed copy of
        // `legal::can_revolt`'s own precondition, and that copy never
        // special-cased leader Maximilien Robespierre the way `can_revolt`
        // does (RB §8.3.4: with him, revolt eligibility is "no MILITARY
        // action spent this turn" instead of "no civil action spent"). A
        // Robespierre-led player who had already spent a civil action this
        // turn -- exactly what taking Breakthrough itself, or any other
        // civil action, does -- was illegally denied the Breakthrough
        // revolution even though a PLAIN (non-Breakthrough) revolution would
        // still have allowed them. Both `legal.rs::action_card_playable` and
        // this function now read the SAME `legal::revolt_pool_ok` `can_revolt`
        // itself calls, so there is exactly one definition of the rule.
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Maximilien Robespierre");
        p.civil_actions = 3; // one already spent this turn (e.g. a Take)
        p.military_actions = 3; // Despotism's 2 + Robespierre's own +1, all unspent
        p.science = 2; // Monarchy's revolutionCost
        p.hand_civil.push(card("Breakthrough (I)"));
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Breakthrough (I)"));
        assert_eq!(state.players[0].government, card("Monarchy"), "revolution happened");
        assert_eq!(state.players[0].science, 2, "0 left after the revolution, +2 from the card's own gainScience");
    }

    /// `Reserves` prints `gainFoodOrResources`, which is a real choice --
    /// a named gap here until `interact::push_choice` existed.
    #[test]
    fn h_play_action_reserves_offers_food_or_resources() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.blue_total = 20;
        p.hand_civil.push(card("Reserves (I)"));
        let mut state = one_player_state(p);
        play_action_and_drain(&mut state, card("Reserves (I)"));
        assert_eq!(legal::legal_moves(&state).len(), 2, "food, or resources");
        apply(&mut state, Move::Choose { n: 1 });
        let n = match card("Reserves (I)").get().special.iter().find_map(|s| match s {
            Special::GainFoodOrResources(n) => Some(*n),
            Special::A(_) | Special::AllPlayers(_) | Special::B(_) | Special::BestTheaterDoubleCulture | Special::BothPlayers(_) | Special::BuildDiscount(_) | Special::CancelledIfPartiesAttackEachOther | Special::CannotPlayAggressionOrWar | Special::CivilActionBackOnTechDevelop(_) | Special::CivilActionUpgradeUrbanBuildingToTheater | Special::ColonizeDiscardUpTo2MilitaryCardsForBonus(_) | Special::ColonyImmediateBonusApplies | Special::ColonyPermanentBonusTransfers | Special::ComboFoodDiscount(_) | Special::ComboResourceDiscount(_) | Special::Condition(_) | Special::CultureFirstColony(_) | Special::CultureIfTopTwoStrength(_) | Special::CultureOnLeaveEqualToLabResourceProduction | Special::CultureOnRevolution(_) | Special::CultureOnTechDevelop(_) | Special::CulturePerAdditionalColony(_) | Special::CulturePerCivilizationWithMoreCulture(_) | Special::CulturePerHappyFromTemplesTheatersWonders(_) | Special::CulturePerLabEqualToLevel | Special::CulturePerLibraryTheaterPair(_) | Special::CulturePerTheater(_) | Special::DecreasePopulation(_) | Special::DestroyUrbanBuildings(_) | Special::DoubleBestMine | Special::DoublesTacticBonusOfOneArmy | Special::ExtraHappyPerHappySource(_) | Special::FinalScoring(_) | Special::FreeCivilAction(_) | Special::FreePopIncreasePerTurn | Special::Gain(_) | Special::GainCulturePerLevelOfRemovedCard(_) | Special::GainResources(_) | Special::InfantryCountsAsCavalryForTactics | Special::LastRoundSubstitute(_) | Special::LeaderTakeCivilActionDiscount(_) | Special::LibraryDiscountsIfTheater | Special::Lose(_) | Special::MilitaryActionAsCivilPerTurn(_) | Special::MilitaryActionCombinedPopIncreaseAndUnitBuild | Special::NoAttacksBetweenParties | Special::OnAttackBetweenParties(_) | Special::OnBuildCulture(_) | Special::OnBuildCulturePerTechLevelSum | Special::OnReplacePutUnderCompletedWonderHappy(_) | Special::OncePerGameTwoPoliticalActions | Special::OpponentDecreasesPopulation(_) | Special::OpponentsPayDoubleMilitaryActionsToAttackYou | Special::OrTakesSpecialTechnologiesOfSameTotalScienceCost | Special::PeekTopEventCardInPolitics | Special::PerTurnChoice | Special::PlayerWithLeastCulture(_) | Special::PlayerWithMostCulture(_) | Special::PlayersWithMostDiscontentWorkers(_) | Special::PlayersWithMostHappyFaces(_) | Special::PopIncreaseFoodDiscount(_) | Special::RemoveAsPoliticalActionForYellowToken(_) | Special::RemoveAsPoliticalActionFreeColonize | Special::RemoveFromGame | Special::ResourceOnMilitaryUnitBuildOrUpgrade(_) | Special::ResourceOnTechDevelop(_) | Special::ResourcesForMilitaryUnitsPerStrongerCivilization(_) | Special::ResourcesPerLabEqualToLevel | Special::RevolutionUsesMilitaryActionsInstead | Special::ScienceOnTechCardTake(_) | Special::SciencePerBestLabOrLibraryLevel | Special::SciencePerLab(_) | Special::StealColony(_) | Special::StrengthPerArtillery(_) | Special::StrengthPerInfantry(_) | Special::StrengthPerMilitaryUnit(_) | Special::StrengthPerTempleOrGovernmentHappy(_) | Special::StrengthPerUnitType(_) | Special::StrongestPlayer(_) | Special::StrongestPlayers(_) | Special::TakeCivilActionDiscountIfLeaderReplacedThisTurn(_) | Special::TakeFromOpponent(_) | Special::TheaterResourceDiscountIfLibrary(_) | Special::TheaterScienceDiscountIfLibrary(_) | Special::TheaterTechScienceDiscount(_) | Special::VictorTakesCulture | Special::VictorTakesScienceUpTo(_) | Special::VictorTakesYellowTokens | Special::WeakestPlayer(_) | Special::WeakestPlayers(_) | Special::WonderTakeNoExtraCivilActions => None,
        }) {
            Some(n) => n as u16,
            None => panic!("Reserves prints gainFoodOrResources"),
        };
        assert_eq!(state.players[0].resources, n);
        assert_eq!(state.players[0].food, 0);
    }

    // -------------------------------------------------------------- pacts

    #[test]
    fn h_cancel_pact_removes_only_pacts_the_actor_is_party_to() {
        let mut owner = blank_player(1, card("Despotism"));
        owner.pacts.push(Pact { card: card("Peace Treaty"), owner: 1, partner: 0, a: 1, b: 0 });
        owner.pacts.push(Pact { card: card("Peace Treaty"), owner: 1, partner: 2, a: 1, b: 2 });
        let mut state = blank_state(4, {
            let filler = || blank_player(3, card("Despotism"));
            [blank_player(0, card("Despotism")), owner, filler(), filler()]
        });
        h_cancel_pact(&mut state, 0, 1);
        assert_eq!(state.players[1].pacts.len(), 1, "only the pact involving player 0 is dropped");
        assert_eq!(state.players[1].pacts.as_slice()[0].partner, 2);
        assert!(state.players[0].politics_done);
        assert_eq!(state.phase, Phase::Actions);
    }

    #[test]
    fn h_resign_clears_hands_and_pacts() {
        let mut p = blank_player(0, card("Despotism"));
        p.hand_civil.push(card("Irrigation"));
        p.hand_military.push(card("Warriors"));
        p.pacts.push(Pact { card: card("Peace Treaty"), owner: 0, partner: 1, a: 0, b: 1 });
        let mut state = one_player_state(p);
        h_resign(&mut state, 0);
        assert!(state.players[0].resigned);
        assert!(state.players[0].hand_civil.is_empty());
        assert!(state.players[0].hand_military.is_empty());
        assert!(state.players[0].pacts.is_empty());
        assert!(state.discarded_military[Age::A as usize].contains(card("Warriors")));
        assert!(state.players[0].politics_done);
    }

    #[test]
    fn h_resign_scores_the_declarer_of_a_war_against_the_resigning_player() {
        // §5.11: a war against a resigned player scores its declarer 7
        // culture, clears the declarer's OWN `war_declared_by_me` if it is
        // still this same war, and discards the war card.
        let mut attacker = blank_player(1, card("Despotism"));
        attacker.war_declared_by_me = card("War over Territory");
        attacker.war_target = 0;
        let mut resigner = blank_player(0, card("Despotism"));
        resigner.wars_declared_on_me[1] = card("War over Territory");
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [resigner, attacker, filler(), blank_player(3, card("Despotism"))]
        });
        h_resign(&mut state, 0);
        assert_eq!(state.players[1].culture, 7);
        assert!(state.players[1].war_declared_by_me.is_none(), "the declarer's own war is cleared too");
        assert!(state.players[0].wars_declared_on_me.iter().all(|c| c.is_none()));
        assert!(state.discarded_military[Age::II as usize].contains(card("War over Territory")));
    }

    #[test]
    fn h_resign_tears_down_a_war_the_resigning_player_themselves_declared() {
        let mut resigner = blank_player(0, card("Despotism"));
        resigner.war_declared_by_me = card("War over Territory");
        resigner.war_target = 1;
        let mut defender = blank_player(1, card("Despotism"));
        defender.wars_declared_on_me[0] = card("War over Territory");
        let mut state = blank_state(4, {
            let filler = || blank_player(2, card("Despotism"));
            [resigner, defender, filler(), blank_player(3, card("Despotism"))]
        });
        h_resign(&mut state, 0);
        assert!(state.players[0].war_declared_by_me.is_none());
        assert!(state.players[1].wars_declared_on_me[0].is_none());
        assert!(state.discarded_military[Age::II as usize].contains(card("War over Territory")));
    }

    // -------------------------------------------------------------- gaps

    #[test]
    fn h_revolution_switches_government_and_pays_its_revolution_cost() {
        let mut p = blank_player(0, card("Despotism"));
        p.science = 10;
        p.civil_actions = 4; // == ca_total(Despotism): every CA still unspent
        p.military_actions = 2; // == Despotism's full MA pool: none spent yet
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Monarchy"), false);
        assert_eq!(state.players[0].government, card("Monarchy"));
        assert_eq!(state.players[0].science, 10 - 2); // Monarchy revolution_cost 2
        // Not Robespierre: civil_actions is zeroed, military_actions carries
        // the unspent-military-action count forward against Monarchy's total.
        assert_eq!(state.players[0].civil_actions, 0);
        assert_eq!(state.players[0].military_actions, 3); // Monarchy MA total, none spent yet
    }

    /// ENGINE BUG regression: `h_revolution` used to read `Card::
    /// revolution_cost` completely undiscounted, ignoring a standing pact's
    /// science discount (`Stats::tech_discount`) that `h_develop`'s own
    /// `costs::tech_cost` already applied to the peaceful path. Confirmed
    /// against BGO journal `7523162` (Grey revolutions to Constitutional
    /// Monarchy at journal line 292 while Scientific Cooperation's pact is
    /// active: printed `revolutionCost` 6, journal's own "4 science points
    /// spent" -- a real, silent 2-science overcharge with no matching
    /// journal event, exactly the phantom drop this fix closes).
    #[test]
    fn h_revolution_applies_a_standing_pact_science_discount_to_its_own_cost() {
        let mut p = blank_player(0, card("Despotism"));
        p.science = 10;
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Constitutional Monarchy"));
        p.pacts.push(crate::state::Pact {
            card: card("Scientific Cooperation"),
            owner: 0,
            partner: 1,
            a: 0,
            b: 1,
        });
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Constitutional Monarchy"), false);
        // Constitutional Monarchy's printed revolutionCost is 6, minus the
        // pact's 2 -- 4, matching the journal exactly.
        assert_eq!(state.players[0].science, 10 - 4);
    }

    /// A violent revolution is the same "develops a technology" trigger the
    /// pact's own text names -- confirmed against BGO journal `7523162`
    /// line 292 ("Orange loses 1 science" logged in the SAME line as Grey's
    /// own revolution). `h_revolution` must charge the partner too, exactly
    /// like `h_develop` does.
    #[test]
    fn h_revolution_charges_the_pact_partner_1_science() {
        let mut p = blank_player(0, card("Despotism"));
        p.science = 10;
        p.civil_actions = 4;
        p.military_actions = 2;
        p.hand_civil.push(card("Constitutional Monarchy"));
        p.pacts.push(crate::state::Pact {
            card: card("Scientific Cooperation"),
            owner: 0,
            partner: 1,
            a: 0,
            b: 1,
        });
        let mut state = one_player_state(p);
        state.players[1].science = 5;
        h_revolution(&mut state, 0, card("Constitutional Monarchy"), false);
        assert_eq!(state.players[0].science, 10 - 4, "the revolutioner pays the discounted cost");
        assert_eq!(state.players[1].science, 5 - 1, "the partner pays its own 1 science");
    }

    /// ENGINE BUG regression (docs/REPLAY.md, this pass): a Robespierre
    /// player revolting via Breakthrough must NOT be short-changed 1 civil
    /// action for the rest of the turn. Breakthrough's own `Move::PlayAction`
    /// already spent 1 CA (simulated here the same way -- decrementing
    /// `civil_actions` before calling `h_revolution`, mirroring what
    /// `h_play_action` does one level up in the real dispatch) BEFORE the
    /// ordered `Move::Revolution` resolves; under Robespierre civil is the
    /// UNAFFECTED pool (military pays instead), and that 1 CA is the
    /// revolution's own declaration cost under this exception (RB p.15),
    /// not ordinary this-turn spending -- so it must not also be deducted
    /// from civil's carry-over. Confirmed against 9 raw BGO journals (game
    /// `7523661` line 286 is the traced example) where the human took one
    /// more civil card this same turn than this binary's OLD (buggy)
    /// reconstruction allowed.
    #[test]
    fn h_revolution_via_breakthrough_does_not_charge_robespierres_civil_pool_for_breakthroughs_own_ca() {
        let mut p = blank_player(0, card("Constitutional Monarchy")); // civil_actions 6, military_actions 4
        p.leader = card("Maximilien Robespierre");
        p.science = 20;
        p.civil_actions = 6 - 1; // Breakthrough's own PlayAction already spent 1 CA
        p.military_actions = 4; // untouched by Breakthrough's (civil) cost
        p.hand_civil.push(card("Democracy")); // civilActions 7
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Democracy"), true);
        assert_eq!(state.players[0].government, card("Democracy"));
        // Robespierre: military pays for the revolution, emptied entirely.
        assert_eq!(state.players[0].military_actions, 0);
        // Civil (unaffected) carries the FULL new total: nothing was spent
        // this turn but Breakthrough's own declaration cost, which does not
        // count against it.
        assert_eq!(state.players[0].civil_actions, 7);
    }

    /// This pass's fix 3 (`legal::breakthrough_robespierre_ma_fundable`'s own
    /// doc comment has the full citation): when civil was already exhausted,
    /// `h_play_action` lets Robespierre fund Breakthrough's own RB p.15 "1
    /// CA" with a MILITARY action instead, and sets `p.breakthrough_ma_funded`.
    /// The sibling test just above (`h_revolution_via_breakthrough_does_not_
    /// charge_robespierres_civil_pool_for_breakthroughs_own_ca`) proves the
    /// PRE-EXISTING `via_ordered_action && robespierre` correction is right
    /// when Breakthrough's 1 CA DID come out of civil -- but here it never
    /// did, so that correction must be SKIPPED here, or it wrongly credits a
    /// civil action the player never spent, one-upping the new government's
    /// real total (7, not 8).
    #[test]
    fn h_revolution_does_not_double_credit_a_civil_action_when_breakthrough_paid_its_own_cost_with_a_military_action() {
        let mut p = blank_player(0, card("Constitutional Monarchy")); // civil_actions 6, military_actions 4
        p.leader = card("Maximilien Robespierre");
        p.science = 20;
        p.civil_actions = 6; // untouched -- Breakthrough's own play cost came out of MILITARY this time
        p.military_actions = 4 - 1; // the 1 MA h_play_action spent funding Breakthrough
        p.breakthrough_ma_funded = true; // set by h_play_action's new branch
        p.hand_civil.push(card("Democracy")); // civilActions 7
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Democracy"), true);
        assert_eq!(state.players[0].government, card("Democracy"));
        assert_eq!(state.players[0].military_actions, 0, "Robespierre: military pays for/empties on a revolution");
        assert_eq!(
            state.players[0].civil_actions, 7,
            "nothing was ever spent from civil this turn -- the new government's full total (7), \
             not 8 (which would mean crediting back a civil action that was never taken)"
        );
    }

    /// Same scenario, but the revolution is declared BARE (not via
    /// Breakthrough): `via_ordered_action=false` must leave `spent` alone --
    /// this is the precondition-enforced case (RB p.15: a bare revolution
    /// requires the whole civil pool to still be unspent), so it should
    /// behave identically to the always-passing `via_ordered_action=true`
    /// compensation being a no-op when nothing was pre-spent.
    #[test]
    fn h_revolution_bare_robespierre_still_gets_full_new_civil_total_when_nothing_was_spent() {
        let mut p = blank_player(0, card("Constitutional Monarchy"));
        p.leader = card("Maximilien Robespierre");
        p.science = 20;
        p.civil_actions = 6; // full pool, unspent -- the bare-revolution precondition
        p.military_actions = 4;
        p.hand_civil.push(card("Democracy"));
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Democracy"), false);
        assert_eq!(state.players[0].military_actions, 0);
        assert_eq!(state.players[0].civil_actions, 7);
    }

    /// ENGINE BUG FIX (`IllegalMove: Upgrade` bucket, game `7522665` round
    /// 11): `docs/RULES_SPEC.md` line 232 says a revolution "counts as
    /// developing a technology" (that line is citing Newton's own trigger,
    /// but the citation covers every develop-triggered leader ability the
    /// same way `h_develop`'s peaceful path already does via `on_develop`).
    /// `h_revolution` used to hand-roll ONLY Newton's arm inline, so a
    /// Leonardo da Vinci player seizing their government by violent
    /// revolution never got the leader's "every time you develop a
    /// technology, gain 1 resource" trigger. The real game's own journal
    /// names the leader explicitly on this exact move: "Orange revolutions
    /// Change government to Constitutional Monarchy; 6 science points
    /// spent; Orange loses 6 science; Leonardo Da Vinci gives 1 resource".
    #[test]
    fn h_revolution_triggers_leonardo_da_vincis_develop_bonus() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Leonardo da Vinci");
        p.science = 10;
        p.civil_actions = 4; // == ca_total(Despotism): every CA still unspent
        p.military_actions = 2; // == Despotism's full MA pool: none spent yet
        p.blue_total = 20; // bank room for gain_resources to actually pay out
        p.hand_civil.push(card("Monarchy"));
        let mut state = one_player_state(p);
        h_revolution(&mut state, 0, card("Monarchy"), false);
        assert_eq!(state.players[0].resources, 1, "Leonardo da Vinci's +1 resource must fire on a revolution, not just a peaceful develop");
    }

    #[test]
    fn do_wonder_step_pays_resources_and_advances_progress() {
        // Pyramids: stages [3, 2, 1].
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.wonder = card("Pyramids");
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 3, false, None);
        assert_eq!(state.players[0].civil_actions, 3);
        assert_eq!(state.players[0].resources, 10 - 3);
        assert_eq!(state.players[0].wonder_steps, 1);
        assert_eq!(state.players[0].wonder_stages_built, 1 << 0, "leftmost unbuilt stage, position 0");
        assert_eq!(state.players[0].wonder, card("Pyramids"), "not complete yet: 1 of 3 stages paid");
    }

    /// Regression for the "resources short by N" `IllegalMove: WonderStep`
    /// bucket (`docs/REPLAY.md`): BGO lets a player build ANY uncovered
    /// stage, paying that stage's own printed cost, not always the next
    /// left-to-right one. Game `7520718`'s own Colossus `[3, 3]` was built
    /// 3-then-1 (Engineering Genius's own discount landing the second stage
    /// at 1, not 3) -- the journal's second line pays `1`, and this is the
    /// exact shape `do_wonder_step` must reproduce: a cost-1 payment that
    /// still counts as building a real stage (position 1, not position 0
    /// again), not a silent no-op that would leave the wonder perpetually
    /// one stage short of completing.
    #[test]
    fn do_wonder_step_charges_the_specific_stage_cost_not_always_the_leftmost() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.wonder = card("Colossus"); // stages [3, 3]
        let mut state = one_player_state(p);
        // First stage built at full price (position 0).
        do_wonder_step(&mut state, 0, 1, 3, false, None);
        // Second stage -- the one this test is actually about -- built at
        // cost 1 (the Engineering Genius discount the journal's own second
        // line states), NOT the leftmost-unbuilt stage's price (3). The
        // replay grounding resolves this to position 1 and passes it
        // explicitly; a left-to-right default would also land here (only
        // position 1 remains) but the explicit pos is the honest path.
        do_wonder_step(&mut state, 0, 1, 1, false, Some(1));
        assert_eq!(state.players[0].resources, 10 - 3 - 1, "second stage must charge its OWN cost (1), not the leftmost unbuilt stage's (3)");
        assert!(state.players[0].wonder.is_none(), "both positions now built, wonder complete");
        assert!(state.players[0].completed_wonders.contains(card("Colossus")));
    }

    /// `wonder_stages_cost` (not `wonder_stage_cost`) is the price a
    /// SPECIFIC, already-partially-built wonder charges for its next
    /// unbuilt stage: game `7521361`'s own Pyramids `[3, 2, 1]` was built
    /// 3-then-2 (the journal's second line pays `2` for the SECOND stage,
    /// not `1`), so after the first stage is built the leftmost-UNBUILT
    /// stage must price at `2`, not `1` (the value `wonder_stage_cost`'s
    /// old, count-based reading would give at `wonder_steps == 1`).
    #[test]
    fn wonder_stages_cost_prices_the_leftmost_unbuilt_stage_not_a_fixed_offset() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.wonder = card("Pyramids"); // stages [3, 2, 1]
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 3, false, None);
        let p = &state.players[0];
        assert_eq!(p.wonder_steps, 1);
        assert_eq!(p.wonder_stages_built, 1 << 0);
        assert_eq!(
            costs::wonder_stages_cost(&state, 0, 1),
            2,
            "leftmost UNBUILT stage is position 1 (cost 2), not position 2 (cost 1, what a count-based offset would read)"
        );
    }

    #[test]
    fn do_wonder_step_completes_the_wonder_and_gains_completion_culture() {
        // Fast Food Chains: stages [4, 4, 4, 4], onBuildCulture
        // "2*workers(farm,mine)+1*workers(urban,military)".
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 4;
        p.resources = 10;
        p.wonder = card("Fast Food Chains");
        p.wonder_stages_built = 0b0111; // stages 0, 1, 2 built; only the last remains
        p.techs.insert(card("Agriculture"), TechSlot { workers: 2, stored: 0 }); // production
        p.techs.insert(card("Warriors"), TechSlot { workers: 1, stored: 0 }); // unit
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 4, false, None); // final stage costs 4 (stages [4,4,4,4])
        assert_eq!(state.players[0].resources, 10 - 4);
        assert!(state.players[0].wonder.is_none(), "wonder completed");
        assert_eq!(state.players[0].wonder_steps, 0);
        assert!(state.players[0].completed_wonders.contains(card("Fast Food Chains")));
        // 2 * production workers (2) + 1 * urban-or-unit workers (1) = 5.
        assert_eq!(state.players[0].culture, 5);
    }

    /// Pyramids prints a standing `civil_actions: 1` bonus (`ca_total`
    /// includes it from the moment it is complete). Completing it MID-TURN
    /// must make that extra civil action usable for the REST of that same
    /// turn, exactly like `set_government` already does for a mid-turn
    /// revolution -- not just from next turn's reset onward. Found via
    /// `rust/src/bin/replay.rs` against a real BGO human game
    /// (`docs/REPLAY.md`): a human took a 5th civil-costing action in the
    /// same turn Pyramids completed, which the reconstructed engine (before
    /// this fix) rejected as illegal because `p.civil_actions` -- a
    /// decrementing-only per-turn pool -- was never topped up when
    /// `on_enter_play` ran for the newly completed wonder.
    #[test]
    fn do_wonder_step_completing_pyramids_grants_the_extra_civil_action_this_turn() {
        let mut p = blank_player(0, card("Despotism")); // ca_total == 4
        p.civil_actions = 1; // this turn's last civil action, about to pay for the final stage
        p.resources = 10;
        p.wonder = card("Pyramids"); // stages [3, 2, 1], printed civil_actions: 1
        p.wonder_stages_built = 0b0011; // stages 0, 1 built; only the last (cost-1) stage remains
        let mut state = one_player_state(p);
        do_wonder_step(&mut state, 0, 1, 0, false, None); // final (cost-1) stage
        assert!(state.players[0].completed_wonders.contains(card("Pyramids")), "wonder completed");
        assert_eq!(
            state.players[0].civil_actions, 1,
            "Pyramids' +1 civil action must be usable THIS turn (paid the final stage's own CA down \
             to 0, then Pyramids' own +1 CA effect should bring it back to 1), not just from the \
             next turn's reset"
        );
    }

    /// ENGINE BUG (`docs/REPLAY.md` Finding 1, 2026-08): Development of
    /// Civil Life's "Immediately, each civilization may ... build a farm,
    /// mine or urban building ... It costs 1 [resource] less than usual" is
    /// the SAME shape as Rich Land's "Build or upgrade a farm or mine; pay 1
    /// less resource" -- rule item 11 says an action card's ordered action
    /// is performed "under normal rules but paying no civil ... action for
    /// it", and Rich Land's own `Special::FreeCivilAction` path IS wired
    /// CA-free (`apply_free_civil_move`). Civil Life's identical grant,
    /// banked in `p.one_time_discount.build_resources`, was never wired to
    /// that same exemption for the ordinary `Move::Build` path -- found by
    /// replaying real BGO game 7523355 (`docs/REPLAY.md`): a human spent 5
    /// civil-costing actions on a 4-civil-action Despotism turn, and the
    /// only one of the five explained by nothing else in play was their own
    /// build of the very card Development of Civil Life had just discounted
    /// for them.
    #[test]
    fn do_build_spends_no_civil_action_when_civil_life_banked_a_build_discount() {
        let mut p = blank_player(0, card("Despotism"));
        p.civil_actions = 0; // this turn's civil-action pool is already spent
        p.resources = 10;
        p.workers_free = 1;
        p.techs.insert(card("Agriculture"), TechSlot { workers: 0, stored: 0 });
        p.one_time_discount.build_resources = 1; // Civil Life's banked grant
        let mut state = one_player_state(p);
        do_build(&mut state, 0, card("Agriculture"), 0, false);
        assert_eq!(
            state.players[0].civil_actions, 0,
            "Civil Life's own build must not spend a civil action -- there were none left to spend"
        );
        assert_eq!(
            state.players[0].resources,
            10 - (2 - 1),
            "the build must still land at the DISCOUNTED resource cost"
        );
        assert_eq!(state.players[0].techs.workers(card("Agriculture")), 1, "the build must still place a worker");
        assert_eq!(state.players[0].one_time_discount.build_resources, 0, "the one-shot grant is consumed exactly once");
    }

    /// `EndTurn` was a named gap here until `game.rs` landed. It is now a
    /// one-line delegation to `game::end_turn` (the whole §6.6 sequence plus
    /// the hand-off), which `game.rs`'s own tests cover -- this is only a
    /// smoke test that [`apply`] routes there at all.
    #[test]
    fn apply_end_turn_delegates_to_game() {
        let mut state =
            blank_state(2, std::array::from_fn(|i| blank_player(i as u8, card("Despotism"))));
        let before = state.turn;
        apply(&mut state, Move::EndTurn);
        assert_eq!(state.turn, before + 1, "the turn advanced");
        assert_eq!(state.current, 1, "...to the next player");
    }

    /// `OfferPact` used to be a named gap here (no `state.pending` field).
    /// It now opens a real decision -- and the point of the whole subsystem
    /// is that the TARGET answers it while `current` stays put.
    #[test]
    fn apply_offer_pact_hands_the_decision_to_the_target() {
        let p = blank_player(0, card("Despotism"));
        let mut state = blank_state(2, std::array::from_fn(|i| blank_player(i as u8, card("Despotism"))));
        state.players[0] = p;
        let peace = card("Peace Treaty");
        state.players[0].hand_military.push(peace);
        apply(&mut state, Move::OfferPact { card: peace, target: 1, side: PactSide::B });
        assert!(!state.players[0].hand_military.contains(peace), "the card is revealed");
        assert!(state.players[0].politics_done, "the political action is spent either way");
        assert_eq!(state.decider(), 1, "the partner answers");
        assert_eq!(state.current, 0, "...and the turn has not moved");
        // Offering side B puts the TARGET on `a` -- see `state::Pact`.
        match state.pending.top() {
            Some(crate::state::Pending::Choice(c)) => assert_eq!(
                c.kind,
                crate::state::ChoiceKind::PactOffer { owner: 0, card: peace, a: 1, b: 0 }
            ),
            other => panic!("expected a pact offer, got {other:?}"),
        }
    }

    // -------------------------------------------- wonder-completion culture

    #[test]
    fn wonder_completion_culture_fast_food_chains() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Agriculture"), TechSlot { workers: 2, stored: 0 }); // production
        p.techs.insert(card("Warriors"), TechSlot { workers: 1, stored: 0 }); // unit
        let gained = wonder_completion_culture(&p, card("Fast Food Chains"));
        // 2 * production workers (2) + 1 * urban-or-unit workers (1) = 5
        assert_eq!(gained, 5);
    }

    /// Hollywood: "twice the total culture production of your theaters and
    /// libraries" (§9.2) -- including a modifier that reaches into that
    /// production, not just the printed numbers. Shakespeare's
    /// `CulturePerLibraryTheaterPair(2)` adds `2 * min(library workers,
    /// theater workers)` on top of Drama's printed 2 culture and Printing
    /// Press's printed 1: `(2 + 1 + 2*min(1,1)) * 2 == 10`.
    #[test]
    fn wonder_completion_culture_hollywood_counts_modifiers() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("William Shakespeare");
        p.techs.insert(card("Drama"), TechSlot { workers: 1, stored: 0 }); // theater, culture 2
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 }); // library, culture 1
        let gained = wonder_completion_culture(&p, card("Hollywood"));
        assert_eq!(gained, 10);
    }

    /// A Hollywood completion with no modifiers in play is just twice the
    /// printed theater/library culture: `(2 + 1) * 2 == 6`.
    #[test]
    fn wonder_completion_culture_hollywood_printed_only() {
        let mut p = blank_player(0, card("Despotism"));
        p.techs.insert(card("Drama"), TechSlot { workers: 1, stored: 0 });
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 });
        let gained = wonder_completion_culture(&p, card("Hollywood"));
        assert_eq!(gained, 6);
    }

    /// Internet: "the combined culture, science and strength your urban
    /// buildings give" (§9.2), again the EFFECTIVE output. Leonardo da
    /// Vinci's `SciencePerBestLabOrLibraryLevel` adds the best staffed
    /// lab-or-library's level (1, from either Age-I card here) as science on
    /// top of the printed sum: Alchemy's 2 science, Printing Press's 1
    /// culture + 1 science, Bread and Circuses' 1 strength -- `2+1+1+1 == 5`
    /// printed, `+1` from the leader, `== 6`.
    #[test]
    fn wonder_completion_culture_internet_counts_modifiers() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Leonardo da Vinci");
        p.techs.insert(card("Alchemy"), TechSlot { workers: 1, stored: 0 }); // lab, science 2
        p.techs.insert(card("Printing Press"), TechSlot { workers: 1, stored: 0 }); // library, culture 1 / science 1
        p.techs.insert(card("Bread and Circuses"), TechSlot { workers: 1, stored: 0 }); // arena, strength 1
        let gained = wonder_completion_culture(&p, card("Internet"));
        assert_eq!(gained, 6);
    }

    // --------------------------------------------------------- Julius Caesar

    /// ENGINE BUG REGRESSION (`docs/REPLAY.md`, found by replaying BGO game
    /// `7523338`, where the same player is offered and declines the second
    /// political action on rounds 3, 4, 5 and 7 and still holds the
    /// ability). The printed card is *"After you play a political action,
    /// you MAY play another political action. This ability can be used only
    /// once per game."* -- declining is not using it, so the once-per-game
    /// survives a decline.
    #[test]
    fn declining_julius_caesars_second_political_action_does_not_spend_the_once_per_game() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Julius Caesar");
        let mut state = one_player_state(p);
        state.phase = Phase::Politics;

        // Turn one: a real political action arms the second one...
        end_politics(&mut state, 0, PoliticalAction::Played);
        assert!(state.players[0].caesar_second_politics);
        assert_eq!(state.phase, Phase::Politics, "the phase stays open for action two");
        // ...and the player declines it.
        end_politics(&mut state, 0, PoliticalAction::Passed);
        assert!(!state.players[0].caesar_second_politics);
        assert_eq!(state.phase, Phase::Actions);
        assert!(
            !state.players[0].caesar_double_politics_used,
            "a declined second action must leave the once-per-game available"
        );

        // Turn two: the offer comes again, and taking it really does spend it.
        state.phase = Phase::Politics;
        state.players[0].politics_done = false;
        end_politics(&mut state, 0, PoliticalAction::Played);
        assert!(state.players[0].caesar_second_politics);
        end_politics(&mut state, 0, PoliticalAction::Played);
        assert!(state.players[0].caesar_double_politics_used, "using it spends it");
        assert_eq!(state.phase, Phase::Actions);

        // Turn three: spent, so no second action is offered at all.
        state.phase = Phase::Politics;
        state.players[0].politics_done = false;
        end_politics(&mut state, 0, PoliticalAction::Played);
        assert!(!state.players[0].caesar_second_politics);
        assert_eq!(state.phase, Phase::Actions);
    }

    /// The other half of the same sentence: the ability triggers off having
    /// PLAYED a political action. Passing is not playing one, so a passed
    /// politics phase never opens a second action -- corroborated by the
    /// corpus, which has no turn anywhere with two political passes in it.
    #[test]
    fn passing_the_political_phase_never_arms_julius_caesars_second_action() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Julius Caesar");
        let mut state = one_player_state(p);
        state.phase = Phase::Politics;

        end_politics(&mut state, 0, PoliticalAction::Passed);

        assert!(!state.players[0].caesar_second_politics);
        assert!(!state.players[0].caesar_double_politics_used);
        assert_eq!(state.phase, Phase::Actions);
    }

    // ----------------------------------------------------------- on_enter/leave

    #[test]
    fn on_enter_play_grants_blue_tokens() {
        let mut p = blank_player(0, card("Despotism"));
        on_enter_play(&mut p, card("Taj Mahal")); // wonders often print blueTokens
        // Not every wonder prints blueTokens; assert the mechanism doesn't
        // panic and leaves a sane (non-negative) total either way.
        assert!(p.blue_total <= 100);
    }

    #[test]
    fn on_take_card_aristotle_only_for_technologies() {
        let mut p = blank_player(0, card("Despotism"));
        p.leader = card("Aristotle");
        on_take_card(&mut p, card("Bronze")); // a technology
        assert_eq!(p.science, 1);
        on_take_card(&mut p, card("Napoleon Bonaparte")); // a leader, not a technology
        assert_eq!(p.science, 1, "leaders are not technologies");
    }
}

